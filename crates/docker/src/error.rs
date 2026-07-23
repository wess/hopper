//! Errors from the Docker layer.
//!
//! The daemon's own message is preserved wherever it sends one — a generic
//! "request failed" is useless when the daemon already said "port is already
//! allocated". Status codes are kept so callers can distinguish the cases
//! Docker overloads (404 missing, 409 conflict//already-in-use, 304 no-op).

use std::fmt;

#[derive(Debug, Clone)]
pub struct DockerError {
    pub message: String,
    pub status: Option<u16>,
    pub kind: ErrorKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// The daemon answered with a non-2xx status.
    Api,
    /// We could not reach the daemon at all.
    Transport,
    /// The daemon answered but the body did not decode.
    Decode,
    /// The request outlived its deadline.
    Timeout,
    /// Denied by the OS (socket permissions, not in the `docker` group).
    Permission,
}

impl DockerError {
    pub fn api(status: u16, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            status: Some(status),
            kind: ErrorKind::Api,
        }
    }

    pub fn transport(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            status: None,
            kind: ErrorKind::Transport,
        }
    }

    pub fn decode(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            status: None,
            kind: ErrorKind::Decode,
        }
    }

    pub fn timeout(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            status: None,
            kind: ErrorKind::Timeout,
        }
    }

    pub fn permission(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            status: None,
            kind: ErrorKind::Permission,
        }
    }

    /// The resource does not exist.
    pub fn is_not_found(&self) -> bool {
        self.status == Some(404)
    }

    /// Docker's 409: name already in use, container already started, network
    /// still has endpoints, volume still referenced.
    pub fn is_conflict(&self) -> bool {
        self.status == Some(409)
    }

    /// Docker's 304: already in the requested state (start on a running
    /// container, stop on a stopped one). Callers usually treat this as success.
    pub fn is_not_modified(&self) -> bool {
        self.status == Some(304)
    }

    /// Whether retrying could plausibly succeed — the engine restarting, a
    /// socket not yet listening.
    pub fn is_retryable(&self) -> bool {
        matches!(self.kind, ErrorKind::Transport | ErrorKind::Timeout)
            || matches!(self.status, Some(500..=599))
    }
}

impl fmt::Display for DockerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for DockerError {}

impl From<std::io::Error> for DockerError {
    fn from(e: std::io::Error) -> Self {
        match e.kind() {
            std::io::ErrorKind::PermissionDenied => DockerError::permission(format!(
                "Permission denied talking to the Docker socket: {e}"
            )),
            std::io::ErrorKind::TimedOut => DockerError::timeout(e.to_string()),
            _ => DockerError::transport(e.to_string()),
        }
    }
}

impl From<hyper::Error> for DockerError {
    fn from(e: hyper::Error) -> Self {
        DockerError::transport(e.to_string())
    }
}

impl From<serde_json::Error> for DockerError {
    fn from(e: serde_json::Error) -> Self {
        DockerError::decode(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, DockerError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_helpers_classify_dockers_overloaded_codes() {
        assert!(DockerError::api(404, "no such container").is_not_found());
        assert!(DockerError::api(409, "name in use").is_conflict());
        assert!(DockerError::api(304, "already started").is_not_modified());
        assert!(!DockerError::api(404, "x").is_conflict());
    }

    #[test]
    fn transport_and_server_errors_are_retryable_but_client_errors_are_not() {
        assert!(DockerError::transport("connection refused").is_retryable());
        assert!(DockerError::timeout("deadline").is_retryable());
        assert!(DockerError::api(503, "unavailable").is_retryable());
        assert!(!DockerError::api(404, "missing").is_retryable());
        assert!(!DockerError::api(400, "bad request").is_retryable());
    }

    #[test]
    fn permission_denied_io_errors_are_classified_not_buried_as_transport() {
        let io = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let e: DockerError = io.into();
        assert_eq!(e.kind, ErrorKind::Permission);
    }
}
