//! Connecting to the daemon over whichever transport the endpoint names.
//!
//! One enum covers unix sockets, TCP, TLS, and Windows named pipes so the
//! client above it never branches on transport.

use crate::endpoint::Endpoint;
use crate::error::{DockerError, Result};
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

pub enum Stream {
    Unix(tokio::net::UnixStream),
    Tcp(tokio::net::TcpStream),
    Tls(Box<tokio_native_tls::TlsStream<tokio::net::TcpStream>>),
    #[cfg(windows)]
    Pipe(tokio::net::windows::named_pipe::NamedPipeClient),
}

/// Open a connection to the daemon.
pub async fn connect(ep: &Endpoint) -> Result<Stream> {
    match ep {
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
        Endpoint::Tcp { host, port, tls } => {
            let tcp = tokio::net::TcpStream::connect((host.as_str(), *port))
                .await
                .map_err(|e| {
                    DockerError::transport(format!("Cannot reach the Docker daemon at {host}:{port}: {e}"))
                })?;
            // Nagle batches the small control writes exec sends; disable it so
            // keystrokes reach the container without a round-trip delay.
            let _ = tcp.set_nodelay(true);
            if !*tls {
                return Ok(Stream::Tcp(tcp));
            }
            let connector = native_tls::TlsConnector::new()
                .map_err(|e| DockerError::transport(format!("TLS setup failed: {e}")))?;
            let connector = tokio_native_tls::TlsConnector::from(connector);
            let stream = connector
                .connect(host, tcp)
                .await
                .map_err(|e| DockerError::transport(format!("TLS handshake with {host} failed: {e}")))?;
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
