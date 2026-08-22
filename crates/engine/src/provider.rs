//! The engine provider abstraction.
//!
//! Hopper either *attaches to* an engine someone else runs or *supplies* one
//! itself. One trait covers both so the rest of the app never branches on
//! which: a provider answers `available()` and `status()`, and a `managed` one
//! also owns `start()` / `stop()`.

use async_trait::async_trait;
use docker::Endpoint;
use model::{EngineResources, EngineStats, EngineStatus, ReclaimResult, RuntimeKind};

#[async_trait]
pub trait Provider: Send + Sync {
    /// Stable identifier, used in settings and in `HOPPER_ENGINE`.
    fn id(&self) -> &'static str;

    /// Human name for the settings picker.
    fn label(&self) -> &'static str;

    /// Whether Hopper owns this engine's lifecycle.
    fn managed(&self) -> bool {
        false
    }

    /// Which client this engine is driven by. Almost every engine speaks the
    /// Engine API; Apple's speaks nothing, and needs its own.
    fn runtime(&self) -> RuntimeKind {
        RuntimeKind::EngineApi
    }

    /// Whether this provider can run on this machine at all.
    async fn available(&self) -> bool;

    /// Where the daemon listens, once the provider knows.
    async fn endpoint(&self) -> Option<Endpoint>;

    /// Current state.
    async fn status(&self) -> EngineStatus;

    /// Bring the engine up. Unmanaged providers report that they cannot.
    async fn start(&self, _resources: EngineResources) -> anyhow::Result<()> {
        anyhow::bail!("This engine is not managed by Hopper, so it cannot be started here.")
    }

    async fn stop(&self) -> anyhow::Result<()> {
        anyhow::bail!("This engine is not managed by Hopper, so it cannot be stopped here.")
    }

    /// Return unused disk to the host. Only a VM-backed engine can.
    async fn reclaim(&self) -> ReclaimResult {
        ReclaimResult {
            ok: false,
            detail: "This engine does not manage its own disk.".into(),
        }
    }

    /// VM-level stats, when there is a VM.
    async fn stats(&self) -> Option<EngineStats> {
        None
    }
}

/// The candidate order for a platform.
///
/// `existing` is always the tail so Hopper keeps working against an engine
/// someone else runs, whatever else fails.
pub fn candidates_for(os: &str) -> Vec<&'static str> {
    match os {
        "macos" => vec!["apple", "existing"],
        "linux" => vec!["linux", "existing"],
        _ => vec!["existing"],
    }
}

/// Resolve the preferred provider id: the `HOPPER_ENGINE` environment variable
/// wins, then a saved setting, then the platform default.
pub fn preferred(env: Option<&str>, setting: Option<&str>, os: &str) -> String {
    fn clean(s: Option<&str>) -> Option<&str> {
        s.map(str::trim).filter(|v| !v.is_empty())
    }
    clean(env)
        .or_else(|| clean(setting))
        .map(str::to_string)
        .unwrap_or_else(|| {
            candidates_for(os)
                .first()
                .copied()
                .unwrap_or("existing")
                .to_string()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macos_leads_with_apple_containers_but_can_fall_back() {
        let order = candidates_for("macos");
        assert_eq!(order.first(), Some(&"apple"));
        assert_eq!(
            order.last(),
            Some(&"existing"),
            "an engine someone else runs must always remain reachable"
        );
    }

    #[test]
    fn linux_prefers_the_native_daemon() {
        assert_eq!(candidates_for("linux"), vec!["linux", "existing"]);
    }

    #[test]
    fn an_unknown_platform_still_gets_the_fallback() {
        assert_eq!(candidates_for("windows"), vec!["existing"]);
        assert_eq!(candidates_for("plan9"), vec!["existing"]);
    }

    #[test]
    fn the_environment_variable_overrides_everything() {
        assert_eq!(preferred(Some("existing"), Some("apple"), "macos"), "existing");
    }

    #[test]
    fn a_saved_setting_beats_the_platform_default() {
        assert_eq!(preferred(None, Some("existing"), "macos"), "existing");
    }

    #[test]
    fn with_no_preference_the_platform_default_wins() {
        assert_eq!(preferred(None, None, "macos"), "apple");
        assert_eq!(preferred(None, None, "linux"), "linux");
    }

    #[test]
    fn blank_preferences_are_ignored_rather_than_selecting_nothing() {
        assert_eq!(preferred(Some("  "), None, "macos"), "apple");
        assert_eq!(preferred(Some(""), Some("   "), "linux"), "linux");
    }
}
