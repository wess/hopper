//! Connecting to the daemon over whichever transport the endpoint names.
//!
//! One enum covers unix sockets, TCP, TLS, and Windows named pipes so the
//! client above it never branches on transport.

use crate::endpoint::Endpoint;
use crate::error::{DockerError, Result};
use rustls_pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer};
use std::fs;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

pub enum Stream {
    #[cfg(unix)]
    Unix(tokio::net::UnixStream),
    Tcp(tokio::net::TcpStream),
    Tls(Box<tokio_rustls::client::TlsStream<tokio::net::TcpStream>>),
    #[cfg(windows)]
    Pipe(tokio::net::windows::named_pipe::NamedPipeClient),
}

/// Build the TLS client Docker users expect from `DOCKER_CERT_PATH`.
///
/// The standard directory may contain `ca.pem`, `cert.pem`, and `key.pem`.
/// When it does not provide a CA, the platform's native roots are used, which
/// keeps ordinary HTTPS endpoints working without Docker-specific files.
fn tls_config() -> Result<rustls::ClientConfig> {
    let cert_dir = std::env::var_os("DOCKER_CERT_PATH").map(std::path::PathBuf::from);
    let mut roots = rustls::RootCertStore::empty();

    if let Some(dir) = &cert_dir {
        let ca = dir.join("ca.pem");
        if ca.is_file() {
            let bytes = fs::read(&ca).map_err(|e| {
                DockerError::transport(format!(
                    "Cannot read Docker CA certificate {}: {e}",
                    ca.display()
                ))
            })?;
            for cert in CertificateDer::pem_slice_iter(&bytes) {
                let cert = cert.map_err(|e| {
                    DockerError::transport(format!("Cannot parse Docker CA certificate: {e}"))
                })?;
                roots.add(cert).map_err(|e| {
                    DockerError::transport(format!("Cannot load Docker CA certificate: {e}"))
                })?;
            }
        }
    }

    if roots.is_empty() {
        for cert in rustls_native_certs::load_native_certs().certs {
            roots.add(cert).map_err(|e| {
                DockerError::transport(format!("TLS certificate setup failed: {e}"))
            })?;
        }
    }
    if roots.is_empty() {
        return Err(DockerError::transport(
            "TLS setup failed: no system or Docker CA certificates were found.",
        ));
    }

    // Name the provider rather than letting rustls infer one from crate
    // features: a dependency can switch on aws-lc-rs beside ring, and
    // `ClientConfig::builder()` panics when both are compiled in.
    let builder = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|e| DockerError::transport(format!("TLS setup failed: {e}")))?
    .with_root_certificates(roots);
    let Some(dir) = cert_dir else {
        return Ok(builder.with_no_client_auth());
    };
    let cert = dir.join("cert.pem");
    let key = dir.join("key.pem");
    if !cert.is_file() && !key.is_file() {
        return Ok(builder.with_no_client_auth());
    }
    let cert_bytes = fs::read(&cert).map_err(|e| {
        DockerError::transport(format!(
            "Cannot read Docker client certificate {}: {e}",
            cert.display()
        ))
    })?;
    let certs = CertificateDer::pem_slice_iter(&cert_bytes)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| {
            DockerError::transport(format!("Cannot parse Docker client certificate: {e}"))
        })?;
    let key_bytes = fs::read(&key).map_err(|e| {
        DockerError::transport(format!(
            "Cannot read Docker client key {}: {e}",
            key.display()
        ))
    })?;
    let key = PrivateKeyDer::pem_slice_iter(&key_bytes)
        .next()
        .transpose()
        .map_err(|e| DockerError::transport(format!("Cannot parse Docker client key: {e}")))?
        .ok_or_else(|| DockerError::transport("Docker client key.pem contains no private key."))?;
    builder
        .with_client_auth_cert(certs, key)
        .map_err(|e| DockerError::transport(format!("TLS client certificate setup failed: {e}")))
}

/// Open a connection to the daemon.
pub async fn connect(ep: &Endpoint) -> Result<Stream> {
    match ep {
        #[cfg(unix)]
        Endpoint::Unix { path } => {
            let sock = tokio::net::UnixStream::connect(path).await.map_err(|e| {
                match e.kind() {
                    std::io::ErrorKind::PermissionDenied => DockerError::permission(format!(
                        "Permission denied opening {path}. Your user may need to be in the `docker` group."
                    )),
                    std::io::ErrorKind::NotFound => DockerError::transport(format!(
                        "No Docker socket at {path}. Is an engine running?"
                    )),
                    _ => DockerError::transport(format!("Cannot reach the Docker socket at {path}: {e}")),
                }
            })?;
            Ok(Stream::Unix(sock))
        }
        #[cfg(not(unix))]
        Endpoint::Unix { path } => Err(DockerError::transport(format!(
            "Unix sockets are not supported on this platform; cannot use {path}."
        ))),
        Endpoint::Tcp { host, port, tls } => {
            let tcp = tokio::net::TcpStream::connect((host.as_str(), *port))
                .await
                .map_err(|e| {
                    DockerError::transport(format!(
                        "Cannot reach the Docker daemon at {host}:{port}: {e}"
                    ))
                })?;
            // Nagle batches the small control writes exec sends; disable it so
            // keystrokes reach the container without a round-trip delay.
            let _ = tcp.set_nodelay(true);
            if !*tls {
                return Ok(Stream::Tcp(tcp));
            }
            let config = tls_config()?;
            let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
            let server_name = rustls::pki_types::ServerName::try_from(host.clone())
                .map_err(|_| DockerError::transport(format!("Invalid TLS server name: {host}")))?;
            let stream = connector.connect(server_name, tcp).await.map_err(|e| {
                DockerError::transport(format!("TLS handshake with {host} failed: {e}"))
            })?;
            Ok(Stream::Tls(Box::new(stream)))
        }
        #[cfg(windows)]
        Endpoint::Npipe { path } => {
            use tokio::net::windows::named_pipe::ClientOptions;
            let pipe = ClientOptions::new().open(path).map_err(|e| {
                DockerError::transport(format!("Cannot open the Docker named pipe at {path}: {e}"))
            })?;
            Ok(Stream::Pipe(pipe))
        }
        #[cfg(not(windows))]
        Endpoint::Npipe { path } => Err(DockerError::transport(format!(
            "Named pipes are Windows-only; cannot use {path} on this platform."
        ))),
    }
}

macro_rules! delegate {
    ($self:ident, $inner:ident, $body:expr) => {
        match $self.get_mut() {
            #[cfg(unix)]
            Stream::Unix($inner) => $body,
            Stream::Tcp($inner) => $body,
            Stream::Tls($inner) => $body,
            #[cfg(windows)]
            Stream::Pipe($inner) => $body,
        }
    };
}

impl AsyncRead for Stream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        delegate!(self, s, Pin::new(s).poll_read(cx, buf))
    }
}

impl AsyncWrite for Stream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        delegate!(self, s, Pin::new(s).poll_write(cx, buf))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        delegate!(self, s, Pin::new(s).poll_flush(cx))
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        delegate!(self, s, Pin::new(s).poll_shutdown(cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tls_config_does_not_depend_on_a_process_default_provider() {
        // Both ring and aws-lc-rs can end up compiled into rustls through other
        // crates' features, and `ClientConfig::builder()` panics when it cannot
        // choose. An error (no roots on a bare CI box) is fine; a panic is not.
        let _ = tls_config();
    }
}
