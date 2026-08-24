//! Choosing a provider and pointing the Docker client at it.

use crate::provider::{candidates_for, is_explicit, preferred, Provider};
use crate::providers::{Existing, Linux, Named};
use docker::client::Client;
use model::{EngineChoice, EngineState, EngineStatus, RuntimeKind};
use std::sync::Arc;

pub struct Registry {
    client: Client,
    providers: Vec<Arc<dyn Provider>>,
    active: std::sync::RwLock<String>,
}

/// Whether landing on this provider should be second-guessed.
///
/// Three ways to answer no: it connected, so it is a real engine; it is the
/// managed one already; or the user named it themselves, and selection does
/// not overrule a person.
fn should_fall_forward(status: &EngineStatus, managed: bool, chosen_by_user: bool) -> bool {
    !status.connected && !managed && !chosen_by_user
}

impl Registry {
    /// Build the registry for this platform.
    ///
    /// The engine Hopper is for comes first where it exists; `existing` is
    /// always the tail so Hopper keeps working against an engine someone else
    /// runs.
    pub fn new(client: Client) -> Self {
        #[allow(unused_mut)]
        let mut providers: Vec<Arc<dyn Provider>> = Vec::new();

        // macOS runs on Apple's runtime. It comes first because on a machine
        // that has it, it is the engine Hopper is for.
        #[cfg(target_os = "macos")]
        providers.push(Arc::new(crate::providers::AppleContainers::new()));

        // Every Docker-compatible daemon Hopper can name, so a machine with
        // more than one installed offers a choice instead of one opaque
        // "existing engine". Registered whether or not they are installed:
        // `available()` re-checks the socket, so one installed later shows up
        // without a restart, and one that is missing can say so.
        let env = crate::daemons::Env::current();
        providers.extend(
            crate::daemons::known(std::env::consts::OS, &env)
                .into_iter()
                .map(|d| Arc::new(Named::new(d, client.clone())) as Arc<dyn Provider>),
        );

        providers.extend([
            // Kept registered but off the candidate list: a settings file that
            // still says `linux` has to resolve to something.
            Arc::new(Linux::new(client.clone())) as Arc<dyn Provider>,
            Arc::new(Existing::new(client.clone())),
        ]);
        Self {
            client,
            providers,
            active: std::sync::RwLock::new("existing".to_string()),
        }
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

    /// The engines this machine could be pointed at, for the settings picker.
    ///
    /// Platform candidates only — Rancher Desktop is not a choice on a server,
    /// and a row saying so would be noise. Unavailable candidates *are* listed,
    /// with the reason: "not installed yet" is a thing to act on, where an
    /// absent row is a thing to wonder about.
    pub async fn choices(&self) -> Vec<EngineChoice> {
        let ids = candidates_for(std::env::consts::OS);
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            let Some(p) = self.get(id) else { continue };
            let available = p.available().await;
            let reason = if available {
                None
            } else {
                // The provider's own status says why far better than we could.
                Some(p.status().await.message)
            };
            // `None` for Apple's runtime, which answers no socket at all. The
            // picker says so in words rather than repeating the engine's name
            // back at itself.
            let endpoint = p.endpoint().await.map(|ep| ep.describe());
            out.push(EngineChoice {
                id: p.id().to_string(),
                label: p.label().to_string(),
                available,
                managed: p.managed(),
                reason,
                endpoint,
            });
        }
        out
    }

    /// Pick the first available provider in preference order, and point the
    /// Docker client at whatever it says.
    pub async fn select(&self, setting: Option<&str>) -> EngineStatus {
        let env = std::env::var("HOPPER_ENGINE").ok();
        let want = preferred(env.as_deref(), setting, std::env::consts::OS);
        let chosen_by_user = is_explicit(env.as_deref(), setting);

        let mut order: Vec<String> = vec![want];
        for id in candidates_for(std::env::consts::OS) {
            if !order.iter().any(|o| o == id) {
                order.push(id.to_string());
            }
        }

        for (rank, id) in order.iter().enumerate() {
            let Some(provider) = self.get(id) else { continue };
            // The engine the user named is selected even when it cannot run:
            // its own status says why, and that is the answer they asked for.
            // Falling through would report on some other engine entirely —
            // "no Docker engine is running" to someone who asked for Apple's.
            let named = chosen_by_user && rank == 0;
            if !named && !provider.available().await {
                continue;
            }
            self.activate(&provider).await;
            let status = provider.status().await;
            // The tail fallback is always "available" — it never disqualifies
            // itself — so landing on it proves nothing is listening. On a
            // platform with an engine of its own, that is the moment to offer
            // that engine rather than report a missing Docker.
            if should_fall_forward(&status, provider.managed(), chosen_by_user) {
                if let Some(managed) = self.fall_forward().await {
                    return managed;
                }
            }
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

    /// Point the Docker client at a provider and record it as the active one.
    ///
    /// One place, so a provider can never become active by half: `select` and
    /// `fall_forward` both land here.
    async fn activate(&self, provider: &Arc<dyn Provider>) {
        if let Some(ep) = provider.endpoint().await {
            self.client.set_endpoint(ep);
        }
        *self.active.write().unwrap() = provider.id().to_string();
    }

    /// The engine Hopper supplies on this platform, if it supplies one.
    fn managed_provider(&self) -> Option<&Arc<dyn Provider>> {
        self.providers.iter().find(|p| p.managed())
    }

    /// The managed provider's own status, if this platform has one — reported
    /// even when it is not the active provider, so the UI can speak to the
    /// engine Hopper could run rather than only the one currently selected.
    async fn managed_status(&self) -> Option<EngineStatus> {
        Some(self.managed_provider()?.status().await)
    }

    /// Select the managed engine when the engine someone else runs is not
    /// there to be attached to.
    ///
    /// Without this, a Mac with neither Docker nor Apple's runtime is told
    /// "no Docker engine found" — an answer about the very thing Hopper exists
    /// to replace, and one with no button on it. Docker is not a requirement
    /// on macOS, so the absence of Docker must lead to Hopper's own engine.
    ///
    /// Only reached when the fallback is down, so a working Docker is never
    /// taken out from under the user.
    async fn fall_forward(&self) -> Option<EngineStatus> {
        let provider = self.managed_provider()?;
        let status = provider.status().await;
        // Hardware that cannot run it has nothing to fall forward to; the
        // fallback's own status, enriched with the reason, reads better.
        if status.state == EngineState::Unsupported {
            return None;
        }
        self.activate(provider).await;
        Some(status)
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

    fn down() -> EngineStatus {
        EngineStatus::new(EngineState::NotInstalled, "existing", "nothing listening")
    }

    #[test]
    fn a_dead_fallback_on_a_platform_with_its_own_engine_is_second_guessed() {
        assert!(should_fall_forward(&down(), false, false));
    }

    #[test]
    fn a_connected_engine_is_left_alone() {
        let up = EngineStatus::new(EngineState::Connected, "existing", "Connected.");
        assert!(!should_fall_forward(&up, false, false));
    }

    #[test]
    fn the_managed_engine_being_down_is_its_own_business() {
        // It is already the engine we would fall forward to.
        assert!(!should_fall_forward(&down(), true, false));
    }

    #[test]
    fn an_engine_the_user_named_is_never_swapped_out_from_under_them() {
        // Someone who set HOPPER_ENGINE=existing wants to see it fail.
        assert!(!should_fall_forward(&down(), false, true));
    }

    #[tokio::test]
    async fn the_picker_offers_this_platforms_engines() {
        let choices = registry().choices().await;
        let ids: Vec<&str> = choices.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, candidates_for(std::env::consts::OS));
        // The engine someone else runs is always choosable — that is what
        // keeps Docker usable for anyone who still wants it.
        let existing = choices.iter().find(|c| c.id == "existing").unwrap();
        assert!(existing.available);
        assert!(!existing.managed);
    }

    #[tokio::test]
    #[cfg(target_os = "macos")]
    async fn a_mac_is_not_offered_the_linux_daemons() {
        let choices = registry().choices().await;
        assert!(!choices.iter().any(|c| c.id == "linux"));
        assert!(choices.iter().any(|c| c.id == "apple" && c.managed));
    }

    #[tokio::test]
    async fn every_named_daemon_is_offered_so_a_machine_with_several_can_choose() {
        // The point of the whole picker: Docker Desktop and Podman installed
        // side by side must be two rows, not one "existing engine".
        let ids: Vec<String> = registry()
            .choices()
            .await
            .into_iter()
            .map(|c| c.id)
            .collect();
        for named in crate::daemons::ids(std::env::consts::OS) {
            assert!(ids.iter().any(|i| i == named), "{named} was not offered");
        }
        assert!(ids.iter().any(|i| i == "existing"));
    }

    #[tokio::test]
    async fn an_engine_that_is_there_says_where_it_listens() {
        // Two Docker-compatible rows both reading "Connected." would not be a
        // choice; the socket is what tells them apart. Apple's runtime is
        // exempt because it answers no socket and is never ambiguous.
        for c in registry()
            .choices()
            .await
            .iter()
            .filter(|c| c.available && !c.managed)
        {
            assert!(
                c.endpoint.is_some(),
                "{} is available but says nothing about where",
                c.id
            );
        }
    }

    #[tokio::test]
    async fn an_unavailable_engine_says_why_rather_than_vanishing() {
        // On Linux that is Apple's runtime; on macOS, the Linux daemons.
        let choices = registry().choices().await;
        for c in choices.iter().filter(|c| !c.available) {
            assert!(c.reason.is_some(), "{} gave no reason", c.id);
        }
    }

    #[tokio::test]
    #[cfg(target_os = "macos")]
    async fn an_engine_the_user_named_is_selected_even_when_it_cannot_run_yet() {
        // Pinning the engine you are moving *to* has to answer about that
        // engine. Falling through would report "no Docker engine is running"
        // to someone who just asked for Apple's runtime.
        let r = registry();
        let status = r.select(Some("apple")).await;
        assert_eq!(status.provider, "apple");
        assert_eq!(r.active_id(), "apple");
    }

    #[tokio::test]
    #[cfg(target_os = "macos")]
    async fn a_mac_falls_forward_to_the_engine_hopper_can_supply() {
        // Docker is not a requirement on macOS, so a fallback with nothing
        // behind it has to lead to Apple's runtime — installed or not.
        //
        // A Mac too old for that runtime is the one case with nowhere to go,
        // and declining is the documented answer. CI runs macOS 14 and a dev
        // machine runs 26, so both halves have to be asserted — but `None` is
        // only allowed for that reason, or a real regression would pass here.
        let r = registry();
        match r.fall_forward().await {
            Some(status) => {
                assert_eq!(status.provider, "apple");
                assert!(status.managed);
                assert_eq!(r.active_id(), "apple", "and it becomes the active provider");
            }
            None => assert!(
                apple::system::too_old().is_some(),
                "a Mac new enough for Apple's runtime must not refuse to fall forward"
            ),
        }
    }

    #[tokio::test]
    #[cfg(not(target_os = "macos"))]
    async fn a_platform_with_no_engine_of_its_own_has_nowhere_to_fall_forward_to() {
        // Docker and Podman on Linux are the user's to install, not Hopper's.
        assert!(registry().fall_forward().await.is_none());
    }

    #[tokio::test]
    async fn selecting_points_the_client_at_the_providers_endpoint() {
        let r = registry();
        r.select(None).await;
        assert!(r.active().is_some());
    }
}
