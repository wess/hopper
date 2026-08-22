//! Choosing a provider and pointing the Docker client at it.

use crate::provider::{candidates_for, preferred, Provider};
use crate::providers::{Existing, Linux};
use docker::client::Client;
use model::{EngineState, EngineStatus, RuntimeKind};
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

        // macOS runs on Apple's runtime. It comes first because on a machine
        // that has it, it is the engine Hopper is for.
        #[cfg(target_os = "macos")]
        providers.push(Arc::new(crate::providers::AppleContainers::new()));

        #[cfg(target_os = "macos")]
        let vz = {
            let vz = Arc::new(crate::vz::provider::Vz::new(client.clone()));
            providers.push(Arc::clone(&vz) as Arc<dyn Provider>);
            Some(vz)
        };

        // Linux uses whichever of Docker or Podman is installed.
        providers.push(Arc::new(Linux::new(client.clone())));
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

    /// Which client the active engine is driven by. The host swaps its
    /// backend on this, so it has to come from the provider rather than be
    /// inferred from the id.
    pub fn active_runtime(&self) -> RuntimeKind {
        self.active().map(|p| p.runtime()).unwrap_or_default()
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
            let status = provider.status().await;
            return self.enrich(status).await;
        }

        // `existing` is always available, so reaching here means the registry
        // was built empty — a programming error, not a user-facing state.
        EngineStatus::new(
            EngineState::NotInstalled,
            "none",
            "No engine provider is available on this platform.",
        )
    }

    /// The active provider's status, enriched (see [`Self::enrich`]) so the UI
    /// gets the whole picture from one call. This is what the status poll uses.
    pub async fn status(&self) -> EngineStatus {
        let Some(active) = self.active() else {
            return EngineStatus::default();
        };
        let status = active.status().await;
        self.enrich(status).await
    }

    /// The managed provider's own status, if this platform has one — reported
    /// even when it is not the active provider, so the UI can speak to the
    /// engine Hopper could run rather than only the one currently selected.
    async fn managed_status(&self) -> Option<EngineStatus> {
        for p in &self.providers {
            if p.managed() {
                return Some(p.status().await);
            }
        }
        None
    }

    /// Fold in guidance about the managed engine when an *unmanaged* engine is
    /// active and down. Without this, a dev build (where Hopper's own engine
    /// can't run) or a Mac with no Docker only ever reports "no Docker" — never
    /// that Hopper has an engine of its own and why it isn't the one answering.
    async fn enrich(&self, status: EngineStatus) -> EngineStatus {
        // A connected engine needs no help; a managed one already speaks for
        // itself (its own status drives the UI).
        if status.connected || status.managed {
            return status;
        }
        let Some(managed) = self.managed_status().await else {
            return status;
        };
        // Only surface the managed engine when it genuinely cannot run here —
        // an entitlement-less dev build, or hardware without the framework.
        // (A managed engine that *could* run would have been selected instead.)
        if managed.connected || managed.state != EngineState::Unsupported {
            return status;
        }
        let reason = match managed.detail {
            Some(detail) => format!("{} {}", managed.message, detail),
            None => managed.message,
        };
        EngineStatus {
            detail: Some(reason),
            ..status
        }
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
    fn apple_containers_is_registered_and_ordered_first_on_macos() {
        let ids = registry().ids();
        assert_eq!(ids.first(), Some(&"apple"));
        assert_eq!(
            ids.last(),
            Some(&"existing"),
            "the fallback must stay reachable even when Apple's runtime cannot run"
        );
    }

    #[test]
    fn an_unknown_provider_id_resolves_to_nothing() {
        assert!(registry().get("does-not-exist").is_none());
    }

    #[tokio::test]
    async fn selecting_an_unavailable_provider_does_not_dead_end() {
        let r = registry();
        // An unknown id is not registered, so selection must fall through to an
        // available provider rather than reporting "none". Where it lands is
        // platform-specific (macOS self-provisions its managed engine, so that
        // one is always available); the invariant is that it lands *somewhere*
        // registered.
        let status = r.select(Some("does-not-exist")).await;
        assert_ne!(status.provider, "none", "selection must not dead-end");
        let active = r.active_id();
        assert!(
            r.ids().iter().any(|id| *id == active),
            "the active provider must be a registered one"
        );
    }

    #[tokio::test]
    async fn selecting_points_the_client_at_the_providers_endpoint() {
        let r = registry();
        r.select(None).await;
        assert!(r.active().is_some());
    }
}
