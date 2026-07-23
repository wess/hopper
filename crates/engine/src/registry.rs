//! Choosing a provider and pointing the Docker client at it.

use crate::provider::{candidates_for, preferred, Provider};
use crate::providers::Existing;
use docker::client::Client;
use model::EngineStatus;
use std::sync::Arc;

pub struct Registry {
    client: Client,
    providers: Vec<Arc<dyn Provider>>,
    active: std::sync::RwLock<String>,
    /// The managed engine, kept concretely as well as behind the trait.
    /// Port forwarding is specific to a VM-backed engine, so it has no place
    /// on the trait — but the caller still needs to reach it.
    #[cfg(target_os = "macos")]
    vz: Option<Arc<crate::vz::provider::Vz>>,
}

impl Registry {
    /// Build the registry for this platform.
    ///
    /// The managed engine comes first where it exists; `existing` is always
    /// the tail so Hopper keeps working against an engine someone else runs.
    pub fn new(client: Client) -> Self {
        #[allow(unused_mut)]
        let mut providers: Vec<Arc<dyn Provider>> = Vec::new();

        #[cfg(target_os = "macos")]
        let vz = {
            let vz = Arc::new(crate::vz::provider::Vz::new(client.clone()));
            providers.push(Arc::clone(&vz) as Arc<dyn Provider>);
            Some(vz)
        };

        providers.push(Arc::new(Existing::new(client.clone())));
        Self {
            client,
            providers,
            active: std::sync::RwLock::new("existing".to_string()),
            #[cfg(target_os = "macos")]
            vz,
        }
    }

    /// The managed engine, when this platform has one.
    #[cfg(target_os = "macos")]
    pub fn vz(&self) -> Option<Arc<crate::vz::provider::Vz>> {
        self.vz.clone()
    }

    pub fn ids(&self) -> Vec<&'static str> {
        self.providers.iter().map(|p| p.id()).collect()
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn Provider>> {
        self.providers.iter().find(|p| p.id() == id).cloned()
    }

    pub fn active_id(&self) -> String {
        self.active.read().unwrap().clone()
    }

    pub fn active(&self) -> Option<Arc<dyn Provider>> {
        self.get(&self.active_id())
    }

    /// Pick the first available provider in preference order, and point the
    /// Docker client at whatever it says.
    pub async fn select(&self, setting: Option<&str>) -> EngineStatus {
        let env = std::env::var("HOPPER_ENGINE").ok();
        let want = preferred(env.as_deref(), setting, std::env::consts::OS);

        let mut order: Vec<String> = vec![want];
        for id in candidates_for(std::env::consts::OS) {
            if !order.iter().any(|o| o == id) {
                order.push(id.to_string());
            }
        }

        for id in &order {
            let Some(provider) = self.get(id) else { continue };
            if !provider.available().await {
                continue;
            }
            if let Some(ep) = provider.endpoint().await {
                self.client.set_endpoint(ep);
            }
            *self.active.write().unwrap() = provider.id().to_string();
            return provider.status().await;
        }

        // `existing` is always available, so reaching here means the registry
        // was built empty — a programming error, not a user-facing state.
        EngineStatus::new(
            model::EngineState::NotInstalled,
            "none",
            "No engine provider is available on this platform.",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use docker::Endpoint;

    fn registry() -> Registry {
        Registry::new(Client::new(Endpoint::Unix {
            path: "/nonexistent-hopper.sock".into(),
        }))
    }

    #[test]
    fn the_fallback_provider_is_always_registered() {
        assert!(registry().ids().contains(&"existing"));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn the_managed_engine_is_registered_and_ordered_first_on_macos() {
        let ids = registry().ids();
        assert_eq!(ids.first(), Some(&"vz"));
        assert_eq!(
            ids.last(),
            Some(&"existing"),
            "the fallback must stay reachable even when the VM cannot run"
        );
    }

    #[test]
    fn an_unknown_provider_id_resolves_to_nothing() {
        assert!(registry().get("does-not-exist").is_none());
    }

    #[tokio::test]
    async fn selection_falls_through_to_the_provider_that_is_available() {
        let r = registry();
        // "vz" is not registered on this build, so selection must not dead-end.
        let status = r.select(Some("vz")).await;
        assert_eq!(r.active_id(), "existing");
        assert_eq!(status.provider, "existing");
    }

    #[tokio::test]
    async fn selecting_points_the_client_at_the_providers_endpoint() {
        let r = registry();
        r.select(None).await;
        assert!(r.active().is_some());
    }
}
