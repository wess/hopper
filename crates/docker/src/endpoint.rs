//! Where the Docker daemon lives.
//!
//! Resolves a connection target from the environment so Hopper works across
//! platforms and against remote daemons:
//!
//! * unix socket — Linux / macOS (the default)
//! * named pipe  — Windows (Docker Desktop's `\\.\pipe\docker_engine`)
//! * tcp         — remote daemons, and the Windows "expose on tcp://" option
//!
//! Precedence: `DOCKER_HOST` → `DOCKER_SOCKET` → per-platform default.
//! Everything here is pure (env and OS are injectable) so it unit-tests
//! without a daemon.

use model::MigrationEndpoint;
use std::collections::HashMap;

const UNIX_DEFAULT: &str = "/var/run/docker.sock";
const WINDOWS_DEFAULT_PIPE: &str = "//./pipe/docker_engine";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Endpoint {
    Unix { path: String },
    Npipe { path: String },
    Tcp { host: String, port: u16, tls: bool },
}

impl Default for Endpoint {
    fn default() -> Self {
        Endpoint::Unix {
            path: UNIX_DEFAULT.into(),
        }
    }
}

/// `npipe:////./pipe/docker_engine` → `\\.\pipe\docker_engine`.
///
/// Windows names pipes with backslashes; we accept the forward-slash URL form
/// and convert.
pub fn normalize_pipe(p: &str) -> String {
    p.replace('/', "\\")
}

fn split_host_port(authority: &str, tls: bool) -> (String, u16) {
    let default_port: u16 = if tls { 2376 } else { 2375 };
    let fallback = |host: &str| {
        (
            if host.is_empty() {
                "localhost".to_string()
            } else {
                host.to_string()
            },
            default_port,
        )
    };

    // IPv6 literal: [::1]:2375
    if let Some(rest) = authority.strip_prefix('[') {
        let Some(end) = rest.find(']') else {
            return fallback(rest);
        };
        let host = &rest[..end];
        let tail = &rest[end + 1..];
        let port = tail
            .strip_prefix(':')
            .and_then(|p| p.parse::<u16>().ok())
            .filter(|p| *p > 0)
            .unwrap_or(default_port);
        let host = if host.is_empty() { "localhost" } else { host };
        return (host.to_string(), port);
    }

    match authority.rfind(':') {
        None => fallback(authority),
        Some(i) => {
            let host = &authority[..i];
            let port = authority[i + 1..]
                .parse::<u16>()
                .ok()
                .filter(|p| *p > 0)
                .unwrap_or(default_port);
            let host = if host.is_empty() { "localhost" } else { host };
            (host.to_string(), port)
        }
    }
}

/// Parse a `DOCKER_HOST` value. Returns `None` for an unrecognized scheme so
/// the caller can fall back rather than connect somewhere wrong.
pub fn parse_docker_host(value: &str, tls_verify: bool) -> Option<Endpoint> {
    let v = value.trim();
    if v.is_empty() {
        return None;
    }

    if let Some(path) = v.strip_prefix("unix://") {
        return Some(Endpoint::Unix {
            path: if path.is_empty() {
                UNIX_DEFAULT.into()
            } else {
                path.to_string()
            },
        });
    }
    if let Some(path) = v.strip_prefix("npipe://") {
        return Some(Endpoint::Npipe {
            path: normalize_pipe(path),
        });
    }
    for (scheme, implies_tls) in [("tcp://", false), ("http://", false), ("https://", true)] {
        if let Some(authority) = v.strip_prefix(scheme) {
            let tls = implies_tls || tls_verify;
            let (host, port) = split_host_port(authority, tls);
            return Some(Endpoint::Tcp { host, port, tls });
        }
    }
    // Bare paths: a unix socket, or a Windows pipe (`//./pipe/…`, `\\.\pipe\…`).
    if v.starts_with("//") || v.starts_with(r"\\") {
        return Some(Endpoint::Npipe {
            path: normalize_pipe(v),
        });
    }
    if v.starts_with('/') {
        return Some(Endpoint::Unix { path: v.into() });
    }
    None
}

fn truthy(s: Option<&String>) -> bool {
    matches!(s.map(String::as_str), Some("1") | Some("true"))
}

/// Resolve the active endpoint from an environment map and platform string.
pub fn resolve_endpoint(env: &HashMap<String, String>, os: &str) -> Endpoint {
    if let Some(host) = env.get("DOCKER_HOST") {
        if let Some(ep) = parse_docker_host(host, truthy(env.get("DOCKER_TLS_VERIFY"))) {
            return ep;
        }
    }
    if let Some(sock) = env.get("DOCKER_SOCKET") {
        return if sock.starts_with("//") || sock.starts_with(r"\\") {
            Endpoint::Npipe {
                path: normalize_pipe(sock),
            }
        } else {
            Endpoint::Unix { path: sock.clone() }
        };
    }
    if os == "windows" {
        Endpoint::Npipe {
            path: normalize_pipe(WINDOWS_DEFAULT_PIPE),
        }
    } else {
        Endpoint::Unix {
            path: UNIX_DEFAULT.into(),
        }
    }
}

/// Resolve from the real process environment and host platform.
pub fn from_env() -> Endpoint {
    let env: HashMap<String, String> = std::env::vars().collect();
    resolve_endpoint(&env, std::env::consts::OS)
}

impl Endpoint {
    /// The HTTP `Host:` header for hand-rolled requests.
    pub fn host_header(&self) -> String {
        match self {
            Endpoint::Tcp { host, port, .. } => format!("{host}:{port}"),
            _ => "localhost".into(),
        }
    }

    /// A human-readable description for the UI and logs.
    pub fn describe(&self) -> String {
        match self {
            Endpoint::Tcp { host, port, tls } => {
                let scheme = if *tls { "https" } else { "http" };
                format!("{scheme}://{host}:{port}")
            }
            Endpoint::Unix { path } => format!("unix:{path}"),
            Endpoint::Npipe { path } => format!("npipe:{path}"),
        }
    }

    /// A `DOCKER_HOST` value for child processes (the bundled compose binary,
    /// the docker CLI) so they target the same engine the client is using.
    pub fn docker_host_value(&self) -> String {
        match self {
            Endpoint::Tcp { host, port, tls } => {
                let scheme = if *tls { "https" } else { "tcp" };
                format!("{scheme}://{host}:{port}")
            }
            Endpoint::Npipe { path } => format!("npipe://{}", path.replace('\\', "/")),
            Endpoint::Unix { path } => format!("unix://{path}"),
        }
    }

    /// The filesystem path, for socket and pipe transports.
    pub fn path(&self) -> Option<&str> {
        match self {
            Endpoint::Unix { path } | Endpoint::Npipe { path } => Some(path),
            Endpoint::Tcp { .. } => None,
        }
    }
}

impl From<MigrationEndpoint> for Endpoint {
    fn from(ep: MigrationEndpoint) -> Self {
        match ep {
            MigrationEndpoint::Unix { path } => Endpoint::Unix { path },
            MigrationEndpoint::Npipe { path } => Endpoint::Npipe { path },
            MigrationEndpoint::Tcp { host, port, tls } => Endpoint::Tcp { host, port, tls },
        }
    }
}

impl From<Endpoint> for MigrationEndpoint {
    fn from(ep: Endpoint) -> Self {
        match ep {
            Endpoint::Unix { path } => MigrationEndpoint::Unix { path },
            Endpoint::Npipe { path } => MigrationEndpoint::Npipe { path },
            Endpoint::Tcp { host, port, tls } => MigrationEndpoint::Tcp { host, port, tls },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn parses_unix_urls() {
        assert_eq!(
            parse_docker_host("unix:///var/run/docker.sock", false),
            Some(Endpoint::Unix {
                path: "/var/run/docker.sock".into()
            })
        );
    }

    #[test]
    fn an_empty_unix_url_falls_back_to_the_default_socket() {
        assert_eq!(
            parse_docker_host("unix://", false),
            Some(Endpoint::Unix {
                path: UNIX_DEFAULT.into()
            })
        );
    }

    #[test]
    fn parses_tcp_with_an_explicit_port() {
        assert_eq!(
            parse_docker_host("tcp://192.168.1.10:2375", false),
            Some(Endpoint::Tcp {
                host: "192.168.1.10".into(),
                port: 2375,
                tls: false
            })
        );
    }

    #[test]
    fn tcp_without_a_port_uses_2375_and_2376_under_tls() {
        assert_eq!(
            parse_docker_host("tcp://box", false),
            Some(Endpoint::Tcp {
                host: "box".into(),
                port: 2375,
                tls: false
            })
        );
        assert_eq!(
            parse_docker_host("tcp://box", true),
            Some(Endpoint::Tcp {
                host: "box".into(),
                port: 2376,
                tls: true
            })
        );
    }

    #[test]
    fn https_implies_tls_even_without_tls_verify() {
        assert_eq!(
            parse_docker_host("https://box:2376", false),
            Some(Endpoint::Tcp {
                host: "box".into(),
                port: 2376,
                tls: true
            })
        );
    }

    #[test]
    fn parses_ipv6_literals() {
        assert_eq!(
            parse_docker_host("tcp://[::1]:2375", false),
            Some(Endpoint::Tcp {
                host: "::1".into(),
                port: 2375,
                tls: false
            })
        );
        // No port after the bracket: fall back to the default.
        assert_eq!(
            parse_docker_host("tcp://[fe80::1]", false),
            Some(Endpoint::Tcp {
                host: "fe80::1".into(),
                port: 2375,
                tls: false
            })
        );
    }

    #[test]
    fn a_bogus_port_falls_back_rather_than_connecting_to_port_zero() {
        assert_eq!(
            parse_docker_host("tcp://box:abc", false),
            Some(Endpoint::Tcp {
                host: "box".into(),
                port: 2375,
                tls: false
            })
        );
        assert_eq!(
            parse_docker_host("tcp://box:0", false),
            Some(Endpoint::Tcp {
                host: "box".into(),
                port: 2375,
                tls: false
            })
        );
    }

    #[test]
    fn parses_named_pipes_in_both_spellings() {
        assert_eq!(
            parse_docker_host("npipe:////./pipe/docker_engine", false),
            Some(Endpoint::Npipe {
                path: r"\\.\pipe\docker_engine".into()
            })
        );
        assert_eq!(
            parse_docker_host("//./pipe/docker_engine", false),
            Some(Endpoint::Npipe {
                path: r"\\.\pipe\docker_engine".into()
            })
        );
    }

    #[test]
    fn parses_bare_socket_paths() {
        assert_eq!(
            parse_docker_host("/var/run/docker.sock", false),
            Some(Endpoint::Unix {
                path: "/var/run/docker.sock".into()
            })
        );
    }

    #[test]
    fn rejects_unknown_schemes_so_the_caller_can_fall_back() {
        assert_eq!(parse_docker_host("ftp://box", false), None);
        assert_eq!(parse_docker_host("", false), None);
        assert_eq!(parse_docker_host("   ", false), None);
        assert_eq!(parse_docker_host("relative/path", false), None);
    }

    #[test]
    fn docker_host_wins_over_docker_socket() {
        let e = env(&[
            ("DOCKER_HOST", "tcp://box:2375"),
            ("DOCKER_SOCKET", "/tmp/other.sock"),
        ]);
        assert_eq!(
            resolve_endpoint(&e, "macos"),
            Endpoint::Tcp {
                host: "box".into(),
                port: 2375,
                tls: false
            }
        );
    }

    #[test]
    fn an_unparseable_docker_host_falls_through_to_docker_socket() {
        let e = env(&[
            ("DOCKER_HOST", "nonsense://x"),
            ("DOCKER_SOCKET", "/tmp/other.sock"),
        ]);
        assert_eq!(
            resolve_endpoint(&e, "macos"),
            Endpoint::Unix {
                path: "/tmp/other.sock".into()
            }
        );
    }

    #[test]
    fn tls_verify_upgrades_a_plain_tcp_host() {
        let e = env(&[
            ("DOCKER_HOST", "tcp://box"),
            ("DOCKER_TLS_VERIFY", "1"),
        ]);
        assert_eq!(
            resolve_endpoint(&e, "linux"),
            Endpoint::Tcp {
                host: "box".into(),
                port: 2376,
                tls: true
            }
        );
    }

    #[test]
    fn platform_defaults_apply_with_an_empty_environment() {
        let e = env(&[]);
        assert_eq!(
            resolve_endpoint(&e, "linux"),
            Endpoint::Unix {
                path: UNIX_DEFAULT.into()
            }
        );
        assert_eq!(
            resolve_endpoint(&e, "windows"),
            Endpoint::Npipe {
                path: r"\\.\pipe\docker_engine".into()
            }
        );
    }

    #[test]
    fn describe_and_docker_host_value_render_each_transport() {
        let unix = Endpoint::Unix {
            path: "/tmp/d.sock".into(),
        };
        assert_eq!(unix.describe(), "unix:/tmp/d.sock");
        assert_eq!(unix.docker_host_value(), "unix:///tmp/d.sock");

        let tcp = Endpoint::Tcp {
            host: "box".into(),
            port: 2376,
            tls: true,
        };
        assert_eq!(tcp.describe(), "https://box:2376");
        assert_eq!(tcp.docker_host_value(), "https://box:2376");

        let pipe = Endpoint::Npipe {
            path: r"\\.\pipe\docker_engine".into(),
        };
        assert_eq!(pipe.docker_host_value(), "npipe:////./pipe/docker_engine");
    }

    #[test]
    fn round_trips_through_the_migration_wire_type() {
        for ep in [
            Endpoint::Unix {
                path: "/tmp/a.sock".into(),
            },
            Endpoint::Npipe {
                path: r"\\.\pipe\x".into(),
            },
            Endpoint::Tcp {
                host: "h".into(),
                port: 1,
                tls: true,
            },
        ] {
            let wire: MigrationEndpoint = ep.clone().into();
            assert_eq!(Endpoint::from(wire), ep);
        }
    }
}
