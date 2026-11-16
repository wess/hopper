//! Which client is answering, and what it can do.
//!
//! Hopper speaks to two kinds of engine. Docker, Podman and any remote daemon
//! answer the Engine API over a socket; Apple's runtime answers nothing at all
//! and is driven by running `container`. An enum rather than a trait because
//! the streaming calls take closures — `FnMut(LogLine) -> bool` is not
//! object-safe, and boxing every callback to pretend otherwise would buy
//! nothing.

use docker::{DockerError, Result};
use model::RuntimeKind;

/// The live backend, resolved per call.
///
/// Resolved rather than stored so the `container` binary is looked up fresh:
/// a user who installs Apple's runtime while Hopper is open should not have to
/// restart the app.
pub enum Backend {
    /// Anything that speaks the Engine API.
    EngineApi,
    /// Apple Containers, with the CLI to drive it.
    #[cfg(target_os = "macos")]
    Apple(apple::Cli),
}

impl Backend {
    pub fn kind(&self) -> RuntimeKind {
        match self {
            Self::EngineApi => RuntimeKind::EngineApi,
            #[cfg(target_os = "macos")]
            Self::Apple(_) => RuntimeKind::Apple,
        }
    }

    /// Resolve the backend for a runtime kind.
    ///
    /// A missing Apple binary remains an Apple operation and fails at process
    /// launch. It must never fall through to an unrelated Docker daemon.
    pub fn resolve(kind: RuntimeKind) -> Self {
        match kind {
            RuntimeKind::EngineApi => Self::EngineApi,
            #[cfg(target_os = "macos")]
            RuntimeKind::Apple => Self::Apple(apple::Cli::locate().unwrap_or_else(|| {
                apple::Cli::at(
                    std::env::var_os("HOPPER_CONTAINER_BIN")
                        .unwrap_or_else(|| "/usr/local/bin/container".into()),
                )
            })),
            #[cfg(not(target_os = "macos"))]
            RuntimeKind::Apple => Self::EngineApi,
        }
    }
}

/// The error for an operation the active engine does not have.
///
/// Phrased for a person: it names the engine and the thing it cannot do, so
/// the UI can show it verbatim.
pub fn unsupported(what: &str) -> DockerError {
    DockerError::api(
        501,
        format!("Apple Containers cannot {what}. Switch to Docker in Settings if you need it."),
    )
}

/// `Err(unsupported(..))`, for the many call sites that only need that.
pub fn refuse<T>(what: &str) -> Result<T> {
    Err(unsupported(what))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_engine_api_backend_resolves_to_itself() {
        assert_eq!(
            Backend::resolve(RuntimeKind::EngineApi).kind(),
            RuntimeKind::EngineApi
        );
    }

    #[test]
    fn refusal_names_the_engine_and_the_operation() {
        let e = unsupported("pause a container");
        assert!(e.message.contains("Apple Containers"));
        assert!(e.message.contains("pause a container"));
        // 501 so callers can tell "not implemented here" from a real failure.
        assert_eq!(e.status, Some(501));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn missing_apple_binary_never_selects_an_unrelated_docker_engine() {
        // A stale override stands in for an uninstall mid-session.
        std::env::set_var("HOPPER_CONTAINER_BIN", "/nonexistent/container");
        assert_eq!(
            Backend::resolve(RuntimeKind::Apple).kind(),
            RuntimeKind::Apple,
            "a missing binary must fail on the selected backend"
        );
        std::env::remove_var("HOPPER_CONTAINER_BIN");
    }
}
