//! Engine lifecycle, VM resources, and host `docker` CLI integration.

use serde::{Deserialize, Serialize};

/// The engine lifecycle as Hopper sees it.
///
/// Richer than a reachable/not boolean so the UI can act: a managed provider
/// can be started, a stopped daemon restarted, a permission problem explained —
/// instead of the user just being told to go open something else.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EngineState {
    /// Reachable and answering `/_ping`.
    Connected,
    /// A managed provider is bringing the engine up (or the first probe is in flight).
    Starting,
    /// An engine is present and known, but not currently listening.
    Stopped,
    /// No engine, and no provider that can supply one here.
    NotInstalled,
    /// The endpoint exists but errors or times out.
    Unreachable,
    /// The socket is there but we are denied (e.g. not in the `docker` group).
    NeedsPermission,
    /// The active provider cannot run on this machine.
    Unsupported,
}

impl EngineState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Connected => "connected",
            Self::Starting => "starting",
            Self::Stopped => "stopped",
            Self::NotInstalled => "notInstalled",
            Self::Unreachable => "unreachable",
            Self::NeedsPermission => "needsPermission",
            Self::Unsupported => "unsupported",
        }
    }
}

/// Whether the daemon is reachable, plus enough context to do something about
/// it. `connected` stays as a derived convenience.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatus {
    pub state: EngineState,
    pub connected: bool,
    /// Human one-liner; also feeds the activity banner.
    pub message: String,
    /// Active provider id ("vz" | "linux" | "existing" | …).
    pub provider: String,
    /// Does Hopper own this engine's lifecycle?
    pub managed: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub detail: Option<String>,
    /// Human description of where we are talking.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub endpoint: Option<String>,
}

impl EngineStatus {
    pub fn new(state: EngineState, provider: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            state,
            connected: state == EngineState::Connected,
            message: message.into(),
            provider: provider.into(),
            managed: false,
            detail: None,
            endpoint: None,
        }
    }

    pub fn managed(mut self, managed: bool) -> Self {
        self.managed = managed;
        self
    }

    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = Some(endpoint.into());
        self
    }
}

impl Default for EngineStatus {
    fn default() -> Self {
        Self::new(EngineState::Starting, "existing", "Connecting…")
    }
}

/// Configurable VM resources for a managed engine. CPU and memory apply when
/// the engine restarts; disk size applies to a freshly created data disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineResources {
    pub cpus: u32,
    pub memory_gib: u32,
    pub disk_gib: u32,
}

impl Default for EngineResources {
    fn default() -> Self {
        Self {
            cpus: 4,
            memory_gib: 4,
            disk_gib: 60,
        }
    }
}

/// Live stats from a managed engine's VM, reported by the in-guest agent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineStats {
    pub mem_total_kb: u64,
    pub mem_avail_kb: u64,
    pub disk_total_kb: u64,
    pub disk_used_kb: u64,
    pub load1: f64,
}

/// Host `docker` CLI compatibility. Hopper can point the user's `docker`
/// command at the active engine through a Docker context named `hopper`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DockerCliStatus {
    pub available: bool,
    pub configured: bool,
    pub host: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub context: Option<String>,
    pub detail: String,
    /// Whether the `docker` on PATH is the one Hopper ships. When Docker
    /// Desktop is uninstalled its CLI goes with it, so Hopper bundles its own
    /// and can install a shim onto PATH.
    #[serde(default)]
    pub bundled: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DockerCliSetupResult {
    pub ok: bool,
    pub detail: String,
    pub status: DockerCliStatus,
}

/// Whether the compatibility socket at `/var/run/docker.sock` points at
/// Hopper. Tools that hardcode that path (Testcontainers, socket-mounting
/// sidecars, CI runners) ignore Docker contexts entirely, so the symlink is
/// the only thing that reaches them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SocketCompatStatus {
    /// Something exists at the well-known path.
    pub present: bool,
    /// That something resolves to Hopper's socket.
    pub ours: bool,
    /// Where it currently points, when it is a symlink.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub target: Option<String>,
    pub detail: String,
}

/// Result of a disk-reclaim request against a managed engine.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReclaimResult {
    pub ok: bool,
    pub detail: String,
}

/// The launch command for Hopper's standalone MCP server, surfaced in Settings
/// so users can register it with an AI client.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpLaunch {
    pub command: String,
    pub args: Vec<String>,
}
