//! Turning a probe result into the engine status the UI renders.
//!
//! The mapping is pure so every branch is testable: a socket that is missing,
//! present but denied, present but silent, and answering all have to produce
//! distinct, actionable messages rather than one "cannot connect".

use docker::{DockerError, ErrorKind};
use model::{EngineState, EngineStatus};

/// Classify a ping failure into the state the UI acts on.
pub fn classify(
    err: &DockerError,
    provider: &str,
    managed: bool,
    endpoint: &str,
) -> EngineStatus {
    let (state, message) = match err.kind {
        ErrorKind::Permission => (
            EngineState::NeedsPermission,
            "Hopper is not allowed to open the Docker socket.".to_string(),
        ),
        ErrorKind::Timeout => (
            EngineState::Unreachable,
            "The Docker engine is not responding.".to_string(),
        ),
        ErrorKind::Transport if managed => (
            EngineState::Stopped,
            "Hopper's engine is not running.".to_string(),
        ),
        ErrorKind::Transport => (
            EngineState::Stopped,
            "No Docker engine is listening.".to_string(),
        ),
        _ => (
            EngineState::Unreachable,
            "The Docker engine returned an unexpected response.".to_string(),
        ),
    };

    EngineStatus::new(state, provider, message)
        .managed(managed)
        .detail(err.message.clone())
        .endpoint(endpoint.to_string())
}

pub fn connected(provider: &str, managed: bool, endpoint: &str, version: &str) -> EngineStatus {
    let message = if version.is_empty() {
        "Connected.".to_string()
    } else {
        format!("Connected to Docker {version}.")
    };
    EngineStatus::new(EngineState::Connected, provider, message)
        .managed(managed)
        .endpoint(endpoint.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_denied_socket_asks_for_permission_rather_than_reporting_it_down() {
        let err = DockerError::permission("denied");
        let s = classify(&err, "existing", false, "unix:/var/run/docker.sock");
        assert_eq!(s.state, EngineState::NeedsPermission);
        assert!(!s.connected);
        // The daemon's own words are kept for the diagnostics line.
        assert_eq!(s.detail.as_deref(), Some("denied"));
    }

    #[test]
    fn a_missing_socket_on_a_managed_engine_offers_to_start_it() {
        let err = DockerError::transport("no such file");
        let s = classify(&err, "vz", true, "unix:/x.sock");
        assert_eq!(s.state, EngineState::Stopped);
        assert!(s.managed, "a managed engine is one the UI can start");
        assert!(s.message.contains("Hopper's engine"));
    }

    #[test]
    fn a_missing_socket_on_an_unmanaged_engine_does_not_claim_ownership() {
        let err = DockerError::transport("no such file");
        let s = classify(&err, "existing", false, "unix:/x.sock");
        assert_eq!(s.state, EngineState::Stopped);
        assert!(!s.message.contains("Hopper's engine"));
    }

    #[test]
    fn a_timeout_is_unreachable_not_stopped() {
        // Something is listening; it just is not answering. Telling the user
        // to start an engine would be wrong.
        let err = DockerError::timeout("deadline exceeded");
        let s = classify(&err, "existing", false, "tcp://box:2375");
        assert_eq!(s.state, EngineState::Unreachable);
    }

    #[test]
    fn connected_status_reports_the_daemon_version() {
        let s = connected("vz", true, "unix:/x.sock", "27.5.1");
        assert_eq!(s.state, EngineState::Connected);
        assert!(s.connected);
        assert!(s.message.contains("27.5.1"));
    }

    #[test]
    fn connected_without_a_version_still_reads_cleanly() {
        let s = connected("existing", false, "unix:/x.sock", "");
        assert_eq!(s.message, "Connected.");
    }
}
