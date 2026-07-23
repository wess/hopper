//! User settings, persisted as JSON under `~/.hopper/`.

use super::engine::EngineResources;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ThemeMode {
    Light,
    Dark,
    #[default]
    System,
}

impl ThemeMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
            Self::System => "system",
        }
    }

    /// Cycle order for the ⌘⇧L shortcut.
    pub fn next(&self) -> Self {
        match self {
            Self::Light => Self::Dark,
            Self::Dark => Self::System,
            Self::System => Self::Light,
        }
    }
}

/// Daemon configuration Hopper writes into the managed engine's `daemon.json`.
///
/// Docker Desktop exposes these under Settings → Docker Engine and Resources →
/// Proxies. Without them, anyone behind a corporate proxy or pulling from a
/// private registry with a self-signed certificate simply cannot use the app.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DaemonConfig {
    #[serde(default)]
    pub insecure_registries: Vec<String>,
    #[serde(default)]
    pub registry_mirrors: Vec<String>,
    #[serde(default)]
    pub dns: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub http_proxy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub https_proxy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub no_proxy: Option<String>,
    /// Free-form JSON merged over the generated document, for keys Hopper has
    /// no dedicated control for. Invalid JSON is reported, never silently
    /// dropped.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub extra_json: Option<String>,
}

impl DaemonConfig {
    pub fn is_empty(&self) -> bool {
        self.insecure_registries.is_empty()
            && self.registry_mirrors.is_empty()
            && self.dns.is_empty()
            && self.http_proxy.is_none()
            && self.https_proxy.is_none()
            && self.no_proxy.is_none()
            && self.extra_json.as_ref().is_none_or(|s| s.trim().is_empty())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(default)]
    pub theme: ThemeMode,
    /// Forced engine provider id, or `None` to auto-select.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub engine_preference: Option<String>,
    #[serde(default)]
    pub resources: EngineResources,
    #[serde(default)]
    pub daemon: DaemonConfig,
    /// Host directories shared into the managed engine's VM.
    ///
    /// The Bun build shared only `$HOME`, and a bind mount outside it resolved
    /// to an empty guest directory with no error — so this list is what makes
    /// `-v /opt/data:/data` behave the way every tutorial assumes.
    #[serde(default)]
    pub shared_paths: Vec<String>,
    /// Start the engine as soon as the app launches.
    #[serde(default = "default_true")]
    pub autostart_engine: bool,
    /// Keep the engine running after the window closes.
    #[serde(default)]
    pub keep_engine_on_quit: bool,
    /// Maintain `/var/run/docker.sock` pointing at Hopper's socket.
    #[serde(default)]
    pub socket_compat: bool,
    /// The active workspace id, or `None` for the built-in "all" scope.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub active_workspace: Option<String>,
}

fn default_true() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemeMode::System,
            engine_preference: None,
            resources: EngineResources::default(),
            daemon: DaemonConfig::default(),
            shared_paths: Vec::new(),
            autostart_engine: true,
            keep_engine_on_quit: false,
            socket_compat: false,
            active_workspace: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_through_json() {
        let s = Settings {
            theme: ThemeMode::Dark,
            shared_paths: vec!["/opt/data".into()],
            ..Default::default()
        };
        let raw = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&raw).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn an_empty_document_reads_as_defaults() {
        let s: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(s, Settings::default());
        // A fresh install must autostart, or first run shows a dead engine.
        assert!(s.autostart_engine);
    }

    #[test]
    fn theme_cycles_through_every_mode() {
        assert_eq!(ThemeMode::Light.next(), ThemeMode::Dark);
        assert_eq!(ThemeMode::Dark.next(), ThemeMode::System);
        assert_eq!(ThemeMode::System.next(), ThemeMode::Light);
    }

    #[test]
    fn daemon_config_emptiness_ignores_whitespace_extra_json() {
        let cfg = DaemonConfig {
            extra_json: Some("  ".into()),
            ..Default::default()
        };
        assert!(cfg.is_empty());
    }
}
