//! Engine selection and supervision, from the UI's point of view.
//!
//! Wraps the provider registry so the app has one place to ask "what engine
//! are we on, is it up, and can I start it" — and so the port forwarder is
//! actually driven, rather than existing and never being called.

use engine::Registry;
use model::{EngineResources, EngineStatus};
use std::sync::Arc;

pub struct Engines {
    registry: Arc<Registry>,
}

impl Engines {
    pub fn new(client: docker::client::Client) -> Self {
        Self {
            registry: Arc::new(Registry::new(client)),
        }
    }

    pub fn registry(&self) -> Arc<Registry> {
        Arc::clone(&self.registry)
    }

    /// Choose a provider and point the Docker client at it.
    pub async fn select(&self, preference: Option<&str>) -> EngineStatus {
        self.registry.select(preference).await
    }

    pub async fn status(&self) -> EngineStatus {
        // Enriched: on a platform with a managed engine, an unmanaged engine
        // that is down still explains why Hopper's own engine isn't running.
        self.registry.status().await
    }

    pub fn active_id(&self) -> String {
        self.registry.active_id()
    }

    /// Whether the active engine is one Hopper can start and stop.
    pub fn managed(&self) -> bool {
        self.registry
            .active()
            .map(|p| p.managed())
            .unwrap_or(false)
    }

    pub async fn start(&self, resources: EngineResources) -> anyhow::Result<()> {
        match self.registry.active() {
            Some(provider) => provider.start(resources).await,
            None => anyhow::bail!("No engine provider is selected."),
        }
    }

    pub async fn stop(&self) -> anyhow::Result<()> {
        match self.registry.active() {
            Some(provider) => provider.stop().await,
            None => Ok(()),
        }
    }

    pub async fn reclaim(&self) -> model::ReclaimResult {
        match self.registry.active() {
            Some(provider) => provider.reclaim().await,
            None => model::ReclaimResult {
                ok: false,
                detail: "No engine is selected.".into(),
            },
        }
    }

    /// Bring forwarded host ports in line with what the running containers
    /// publish.
    ///
    /// Only the managed engine forwards anything: against an engine someone
    /// else runs, published ports are already bound on the host and opening
    /// our own listener would collide with it.
    #[cfg(target_os = "macos")]
    pub async fn resync_forwards(&self) -> Vec<String> {
        let Some(provider) = self.registry.active() else {
            return Vec::new();
        };
        if provider.id() != "vz" {
            return Vec::new();
        }
        // The registry keeps the concrete provider alongside the trait object,
        // because forwarding belongs to a VM-backed engine rather than to
        // every engine.
        let Some(vz) = self.registry.vz() else {
            return Vec::new();
        };
        vz.resync_forwards()
            .await
            .into_iter()
            .map(|(_, reason)| reason)
            .collect()
    }

    #[cfg(not(target_os = "macos"))]
    pub async fn resync_forwards(&self) -> Vec<String> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use docker::{Client, Endpoint};

    fn engines() -> Engines {
        Engines::new(Client::new(Endpoint::Unix {
            path: "/nonexistent-hopper.sock".into(),
        }))
    }

    #[tokio::test]
    async fn selection_always_lands_on_a_provider() {
        let e = engines();
        let status = e.select(None).await;
        assert!(!e.active_id().is_empty());
        assert!(!status.provider.is_empty());
    }

    #[tokio::test]
    async fn an_unknown_preference_falls_back_rather_than_leaving_nothing_selected() {
        let e = engines();
        e.select(Some("no-such-engine")).await;
        assert!(!e.active_id().is_empty());
    }

    #[tokio::test]
    async fn forwarding_is_a_no_op_against_an_engine_hopper_does_not_manage() {
        let e = engines();
        // Force the fallback: its ports are already bound on the host, so
        // opening our own listeners would collide.
        e.select(Some("existing")).await;
        assert!(e.resync_forwards().await.is_empty());
    }
}
