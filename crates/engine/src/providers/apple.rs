//! Apple Containers: the macOS engine.
//!
//! Hopper does not own the VM here — Apple's `container-apiserver` does — but
//! it does own the *services*, so this is a managed provider: it can start and
//! stop them, and it reports what a user needs to do when the runtime is not
//! installed yet.
//!
//! There is no endpoint. Apple exposes no Docker socket, so `endpoint()` is
//! `None` and the host talks to this engine through the `apple` crate instead
//! of the Engine API client.

use apple::Cli;
use async_trait::async_trait;
use docker::Endpoint;
use model::{EngineResources, EngineState, EngineStatus, RuntimeKind};

use crate::provider::Provider;

pub struct AppleContainers;

impl AppleContainers {
    pub fn new() -> Self {
        Self
    }
}

impl Default for AppleContainers {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for AppleContainers {
    fn id(&self) -> &'static str {
        "apple"
    }

    fn label(&self) -> &'static str {
        "Apple Containers"
    }

    fn managed(&self) -> bool {
        true
    }

    fn runtime(&self) -> RuntimeKind {
        RuntimeKind::Apple
    }

    /// Only offered where it can actually run: macOS 26 or later, with the
    /// `container` binary present. A Mac without it falls through to whatever
    /// engine is already running, and the engine view offers the install.
    async fn available(&self) -> bool {
        cfg!(target_os = "macos") && apple::system::too_old().is_none() && apple::installed()
    }

    async fn endpoint(&self) -> Option<Endpoint> {
        None
    }

    async fn status(&self) -> EngineStatus {
        let base = |state: EngineState, message: &str| {
            EngineStatus::new(state, "apple", message)
                .managed(true)
                .endpoint("Apple Containers".to_string())
        };

        if !cfg!(target_os = "macos") {
            return base(
                EngineState::Unsupported,
                "Apple Containers only runs on macOS.",
            );
        }
        if let Some(e) = apple::system::too_old() {
            return base(EngineState::Unsupported, &e.message);
        }
        let Some(cli) = Cli::locate() else {
            return base(
                EngineState::NotInstalled,
                "Apple Containers is not installed yet.",
            )
            .detail(
                "Hopper can download Apple's signed installer for you; macOS will ask you to approve it."
                    .to_string(),
            );
        };

        match apple::system::status(&cli).await {
            Ok(status) if status.running() => {
                let mut s = base(EngineState::Connected, "Connected.");
                if !status.api_server_version.is_empty() {
                    s = s.detail(format!("container {}", status.api_server_version));
                }
                s
            }
            Ok(_) => base(
                EngineState::Stopped,
                "Apple's container services are not running.",
            ),
            Err(e) => crate::status_from(&e, "apple", true, "Apple Containers"),
        }
    }

    /// Apple sizes each container's VM per run, so there is nothing global to
    /// apply — the resources argument is deliberately ignored.
    async fn start(&self, _resources: EngineResources) -> anyhow::Result<()> {
        let cli = Cli::locate()
            .ok_or_else(|| anyhow::anyhow!("Apple Containers is not installed."))?;
        apple::system::start(&cli)
            .await
            .map_err(|e| anyhow::anyhow!("{}", e.message))
    }

    async fn stop(&self) -> anyhow::Result<()> {
        let cli = Cli::locate()
            .ok_or_else(|| anyhow::anyhow!("Apple Containers is not installed."))?;
        apple::system::stop(&cli)
            .await
            .map_err(|e| anyhow::anyhow!("{}", e.message))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_identifies_as_a_managed_apple_runtime() {
        let p = AppleContainers::new();
        assert_eq!(p.id(), "apple");
        assert!(p.managed(), "Hopper starts and stops Apple's services");
        assert_eq!(p.runtime(), RuntimeKind::Apple);
    }

    #[tokio::test]
    async fn it_offers_no_endpoint_because_apple_publishes_no_socket() {
        // The Engine API client must never be pointed at this provider: there
        // is nothing listening to point it at.
        assert!(AppleContainers::new().endpoint().await.is_none());
    }

    #[tokio::test]
    #[cfg(not(target_os = "macos"))]
    async fn off_macos_it_is_unsupported_rather_than_merely_missing() {
        let s = AppleContainers::new().status().await;
        assert_eq!(s.state, EngineState::Unsupported);
    }

    #[tokio::test]
    #[cfg(target_os = "macos")]
    async fn on_macos_it_reports_something_actionable() {
        // Whatever this machine has, the status must be one the engine view
        // can act on rather than a blank.
        let s = AppleContainers::new().status().await;
        assert!(s.managed);
        assert!(!s.message.is_empty());
        assert!(matches!(
            s.state,
            EngineState::Connected
                | EngineState::Stopped
                | EngineState::NotInstalled
                | EngineState::Unsupported
                | EngineState::Unreachable
        ));
    }
}
