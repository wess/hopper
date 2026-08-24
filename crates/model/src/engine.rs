//! Engine lifecycle, VM resources, and host `docker` CLI integration.

use serde::{Deserialize, Serialize};

/// Which client speaks to the engine.
///
/// Docker, Podman and Hopper's own daemon all answer the Engine API over a
/// socket. Apple's runtime answers nothing — it has no socket at all — so the
/// two are driven by different code and the host has to know which it holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeKind {
    /// Anything that speaks the Docker Engine API.
    #[default]
    EngineApi,
    /// Apple Containers, driven through the `container` CLI.
    Apple,
}

impl RuntimeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::EngineApi => "engineApi",
            Self::Apple => "apple",
        }
    }
}

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
    /// Active provider id ("apple" | "docker" | "podman" | "existing" | …).
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

/// One engine the user can pick in Settings.
///
/// Hopper does not require Docker on macOS, but plenty of people have it — and
/// someone mid-move keeps both for a while. The picker is how they choose, and
/// how the "switch to Docker" refusals from Apple's runtime become an action
/// rather than an instruction with nowhere to go.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineChoice {
    pub id: String,
    pub label: String,
    /// Present and usable on this machine right now.
    pub available: bool,
    /// Hopper owns its lifecycle, so it can be installed and started here.
    pub managed: bool,
    /// Why it cannot be chosen, when it cannot.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reason: Option<String>,
    /// Where it listens, when it is there to listen. A machine with several
    /// Docker-compatible engines installed needs the socket to tell them
    /// apart — two rows both saying "Connected." are not a choice.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub endpoint: Option<String>,
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

/// What the active engine can actually do.
///
/// The Engine API and Apple's runtime are not the same shape: Apple has no
/// pause, no rename, no post-create resource update, no event stream and no
/// healthchecks. Carrying that as data lets the UI hide what is not there
/// instead of offering a button that always fails.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineCapabilities {
    /// Pause and unpause a running container.
    pub pause: bool,
    /// Rename a container in place.
    pub rename: bool,
    /// Change resource limits after creation.
    pub update: bool,
    /// List processes inside a container.
    pub top: bool,
    /// A live event stream. Without it, lists are refreshed by polling.
    pub events: bool,
    /// An interactive shell.
    pub exec: bool,
    /// Browse and edit the container filesystem.
    pub files: bool,
    /// Live CPU/memory samples.
    pub stats: bool,
    /// Healthchecks, and therefore a health state worth showing.
    pub health: bool,
    /// Compose stacks driven by a compose CLI.
    pub compose: bool,
    /// Build images from a Dockerfile.
    pub build: bool,
    /// Restart policies on `run`.
    pub restart_policy: bool,
}

impl EngineCapabilities {
    /// Everything, as the Docker Engine API offers it. Podman answers the same
    /// API and is treated the same.
    pub const fn engine_api() -> Self {
        Self {
            pause: true,
            rename: true,
            update: true,
            top: true,
            events: true,
            exec: true,
            files: true,
            stats: true,
            health: true,
            compose: true,
            build: true,
            restart_policy: true,
        }
    }

    /// Apple Containers, as of `container` 1.2.
    ///
    /// This is what Hopper *does*, not what Apple could do. `container` has
    /// `exec`, `stats` and `cp`, but Hopper does not drive them yet — and
    /// claiming otherwise would be worse than saying no, because the call
    /// would fall through to whatever socket the Engine API client happens to
    /// be holding and read a different daemon.
    pub const fn apple() -> Self {
        Self {
            pause: false,
            rename: false,
            update: false,
            top: false,
            // No event stream at all — the UI polls instead.
            events: false,
            // Needs a hijacked socket; not implemented here yet.
            exec: false,
            files: false,
            stats: false,
            // Each container is its own VM; there is no healthcheck runner.
            health: false,
            // Apple ships no compose, and there is no Docker socket for the
            // real one to talk to. Hopper reads the file and runs the services
            // itself, so stacks work here.
            compose: true,
            // `container build` exists, but Hopper's build path is Engine API.
            build: false,
            restart_policy: false,
        }
    }

    pub fn for_runtime(kind: RuntimeKind) -> Self {
        match kind {
            RuntimeKind::EngineApi => Self::engine_api(),
            RuntimeKind::Apple => Self::apple(),
        }
    }
}

impl Default for EngineCapabilities {
    fn default() -> Self {
        Self::engine_api()
    }
}

#[cfg(test)]
mod capability_tests {
    use super::*;

    #[test]
    fn apple_is_honest_about_what_it_cannot_do() {
        let c = EngineCapabilities::apple();
        assert!(!c.pause, "Apple's runtime has no pause");
        assert!(!c.rename);
        assert!(!c.events, "no event stream means the UI must poll");
        assert!(!c.health, "no healthcheck runner, so no health dot");
        assert!(!c.restart_policy);
    }

    #[test]
    fn apple_claims_nothing_hopper_has_not_implemented() {
        // Claiming a capability Hopper does not drive is worse than refusing
        // it: the call falls through to the Engine API client, which on this
        // backend points at some *other* daemon.
        let c = EngineCapabilities::apple();
        assert!(!c.exec && !c.stats && !c.files && !c.build);
    }

    #[test]
    fn the_engine_api_offers_everything() {
        let c = EngineCapabilities::engine_api();
        assert!(c.pause && c.rename && c.update && c.top && c.events && c.health && c.compose);
    }

    #[test]
    fn capabilities_follow_the_runtime_kind() {
        assert_eq!(
            EngineCapabilities::for_runtime(RuntimeKind::Apple),
            EngineCapabilities::apple()
        );
        assert_eq!(
            EngineCapabilities::for_runtime(RuntimeKind::EngineApi),
            EngineCapabilities::engine_api()
        );
    }
}
