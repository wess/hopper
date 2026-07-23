//! Engine providers: attaching to an engine, or supplying one.

pub mod backoff;
pub mod cli;
pub mod provider;
pub mod providers;
pub mod registry;
pub mod socket;

#[cfg(target_os = "macos")]
pub mod vz;

pub use provider::Provider;
pub use registry::Registry;

use docker::{DockerError, ErrorKind};
use model::{EngineState, EngineStatus};

/// Classify a probe failure into an actionable status.
///
/// Shared by every provider so a denied socket, a silent daemon, and a missing
/// one never collapse into the same "cannot connect".
pub fn status_from(
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
