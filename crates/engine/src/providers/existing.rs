//! The fallback provider: whatever engine is already running.
//!
//! Always available, never managed. This is what keeps Hopper useful as a
//! plain client against Docker Desktop, Colima, Rancher Desktop, a remote
//! daemon over TCP, or a Linux host daemon.

use async_trait::async_trait;
use docker::client::Client;
use docker::Endpoint;
use model::{EngineState, EngineStatus};

use crate::provider::Provider;

pub struct Existing {
    client: Client,
}

impl Existing {
    pub fn new(client: Client) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Provider for Existing {
    fn id(&self) -> &'static str {
        "existing"
    }

    fn label(&self) -> &'static str {
        "Existing engine"
    }

    async fn available(&self) -> bool {
        // The tail fallback must never disqualify itself, or the app would be
        // left with no provider at all.
        true
    }

    async fn endpoint(&self) -> Option<Endpoint> {
        Some(docker::endpoint::from_env())
    }

    async fn status(&self) -> EngineStatus {
        let endpoint = self.client.endpoint();
        let described = endpoint.describe();
        match self.client.ping().await {
            Ok(()) => EngineStatus::new(EngineState::Connected, "existing", "Connected.")
                .endpoint(described),
            Err(e) => {
                let mut status = crate::status_from(&e, "existing", false, &described);
                // Nothing here can be started by Hopper, so say what the user
                // can actually do instead of offering a dead button.
                if status.state == EngineState::Stopped {
                    status.message =
                        "No Docker engine is running. Start one, or let Hopper provide its own."
                            .into();
                    status.state = EngineState::NotInstalled;
                    status.connected = false;
                }
                status
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(path: &str) -> Existing {
        Existing::new(Client::new(Endpoint::Unix {
            path: path.to_string(),
        }))
    }

    #[tokio::test]
    async fn the_fallback_is_always_available_and_never_managed() {
        let p = provider("/nonexistent.sock");
        assert!(p.available().await);
        assert!(!p.managed());
        assert_eq!(p.id(), "existing");
    }

    #[tokio::test]
    async fn a_missing_engine_reads_as_not_installed_rather_than_stopped() {
        // "Stopped" would imply Hopper could start it, which it cannot.
        let p = provider("/nonexistent-hopper-test.sock");
        let status = p.status().await;
        assert_eq!(status.state, EngineState::NotInstalled);
        assert!(!status.connected);
        assert!(status.message.contains("Start one"));
    }

    #[tokio::test]
    async fn starting_an_unmanaged_engine_is_refused_with_a_reason() {
        let p = provider("/nonexistent.sock");
        let err = p.start(Default::default()).await.unwrap_err();
        assert!(err.to_string().contains("not managed"));
    }
}
