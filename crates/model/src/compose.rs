//! Compose stacks: projects reconstructed from container labels, lifecycle
//! actions, the plan a compose file resolves to, and the output a run streams
//! back.

use super::container::{ContainerState, Port, RunInput};
use serde::{Deserialize, Serialize};

/// One line of output from a compose run, streamed to the UI. `done` marks the
/// final frame; `error` is set when the run exited non-zero.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposeProgress {
    pub request_id: String,
    pub line: String,
    pub stream: super::stream::StreamKind,
    pub done: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ComposeStackStatus {
    Running,
    Partial,
    Stopped,
}

impl ComposeStackStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Partial => "partial",
            Self::Stopped => "stopped",
        }
    }
}

/// One container instance belonging to a compose service. A service may have
/// several when scaled.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposeService {
    pub service: String,
    pub container_id: String,
    pub container_name: String,
    pub state: ContainerState,
    /// Human "Up 3 hours".
    pub status: String,
    pub image: String,
    pub ports: Vec<Port>,
}

/// A compose project (stack), reconstructed from container labels so it works
/// against any engine without a compose CLI present.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposeProject {
    pub name: String,
    pub status: ComposeStackStatus,
    pub running: usize,
    pub total: usize,
    pub services: Vec<ComposeService>,
    /// `com.docker.compose.project.config_files`
    pub config_files: Vec<String>,
    /// `com.docker.compose.project.working_dir`
    pub working_dir: Option<String>,
}

impl ComposeProject {
    /// Roll per-container states up into one stack badge.
    pub fn status_for(running: usize, total: usize) -> ComposeStackStatus {
        if total == 0 || running == 0 {
            ComposeStackStatus::Stopped
        } else if running == total {
            ComposeStackStatus::Running
        } else {
            ComposeStackStatus::Partial
        }
    }

    /// The distinct service names in the stack, in first-seen order.
    pub fn service_names(&self) -> Vec<String> {
        let mut seen: Vec<String> = Vec::new();
        for svc in &self.services {
            if !seen.iter().any(|s| s == &svc.service) {
                seen.push(svc.service.clone());
            }
        }
        seen
    }
}

/// A compose file turned into something an engine can actually run.
///
/// Hopper implements Compose itself rather than shelling out to it — on macOS
/// the engine publishes no Docker socket, so there is nothing for the real
/// `docker compose` to talk to. This is the handover: everything resolved,
/// ordered, and named the way Compose names it, so a stack Hopper brings up is
/// the same stack `docker compose` would find.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposePlan {
    pub project: String,
    /// Where relative paths in the file resolved against.
    pub working_dir: String,
    pub config_files: Vec<String>,
    pub networks: Vec<ComposeNetwork>,
    pub volumes: Vec<ComposeVolume>,
    /// Services in the order they have to start.
    pub services: Vec<ComposePlanService>,
    /// File-level notes: an unset variable, a key Hopper does not implement.
    #[serde(default)]
    pub warnings: Vec<String>,
}

impl ComposePlan {
    /// The services that will actually be started, given the active profiles.
    ///
    /// A service in a profile nobody asked for is not part of this run, and a
    /// blocked one cannot be part of any.
    pub fn runnable(&self) -> Vec<&ComposePlanService> {
        self.services
            .iter()
            .filter(|s| s.blocked.is_none() && s.selected)
            .collect()
    }

    /// Every warning in the plan, file-level and per-service, ready to show.
    pub fn all_warnings(&self) -> Vec<String> {
        let mut out = self.warnings.clone();
        for s in &self.services {
            for w in &s.warnings {
                out.push(format!("{}: {w}", s.service));
            }
            if let Some(blocked) = &s.blocked {
                out.push(format!("{}: {blocked}", s.service));
            }
        }
        out
    }
}

/// A network the stack needs. `external` ones are expected to exist already.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposeNetwork {
    pub name: String,
    #[serde(default)]
    pub external: bool,
    #[serde(default)]
    pub internal: bool,
}

/// A named volume the stack needs.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposeVolume {
    pub name: String,
    #[serde(default)]
    pub external: bool,
}

/// One service, resolved down to a container Hopper knows how to create.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposePlanService {
    pub service: String,
    pub run: RunInput,
    /// Services that must be up first, already resolved to real names.
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub profiles: Vec<String>,
    /// Whether the active profiles select this service for the run.
    #[serde(default)]
    pub selected: bool,
    /// Networks beyond the one the container is created on. An engine that
    /// attaches only one at create time reports the rest here.
    #[serde(default)]
    pub extra_networks: Vec<String>,
    /// Why this service cannot start here at all, if it cannot.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub blocked: Option<String>,
    /// What was asked for that this engine will not do.
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// A lifecycle action over a whole stack. `Remove` is a full teardown
/// (down + volumes + orphans).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ComposeAction {
    Up,
    Down,
    Start,
    Stop,
    Restart,
    Remove,
    /// `compose pull` — refresh every service image.
    Pull,
    /// `compose build` on its own, rather than folded into `up --build`.
    Build,
}

impl ComposeAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Down => "down",
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
            Self::Remove => "remove",
            Self::Pull => "pull",
            Self::Build => "build",
        }
    }

    /// Whether the action tears resources down, so the UI can confirm first.
    pub fn is_destructive(&self) -> bool {
        matches!(self, Self::Down | Self::Remove)
    }

    /// `up`, `pull` and `build` need the compose file(s); the rest work
    /// label-driven from just a project name.
    pub fn needs_files(&self) -> bool {
        matches!(self, Self::Up | Self::Pull | Self::Build)
    }
}

/// Which files / project / env a compose command targets.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposeTarget {
    /// `-f`, repeatable.
    #[serde(default)]
    pub files: Vec<String>,
    /// `-p`
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub project: Option<String>,
    /// `--env-file`
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub env_file: Option<String>,
}

/// Extra flags for up/down.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposeOptions {
    /// `--profile`, repeatable.
    #[serde(default)]
    pub profiles: Vec<String>,
    /// `up --build`
    #[serde(default)]
    pub build: bool,
    #[serde(default)]
    pub force_recreate: bool,
    /// `up`/`down --remove-orphans`. `up` defaults this on.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub remove_orphans: Option<bool>,
    /// `down --volumes`
    #[serde(default)]
    pub volumes: bool,
    /// `down --rmi all|local`
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub rmi: Option<String>,
    /// Restrict the action to these services. Empty means the whole project.
    #[serde(default)]
    pub services: Vec<String>,
    /// `up --scale svc=n`
    #[serde(default)]
    pub scale: Vec<ComposeScale>,
}

/// One `--scale service=count` pair.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposeScale {
    pub service: String,
    pub count: u32,
}

/// Result of validating a compose file set (`docker compose config`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposeConfigResult {
    pub ok: bool,
    /// Normalized, merged config on success.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub yaml: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub error: Option<String>,
}

/// Reading or writing a compose file on the host filesystem (the in-app editor).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposeFileResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stack_status_rolls_up_from_counts() {
        assert_eq!(
            ComposeProject::status_for(0, 3),
            ComposeStackStatus::Stopped
        );
        assert_eq!(
            ComposeProject::status_for(2, 3),
            ComposeStackStatus::Partial
        );
        assert_eq!(
            ComposeProject::status_for(3, 3),
            ComposeStackStatus::Running
        );
        // An empty stack is stopped, not running.
        assert_eq!(
            ComposeProject::status_for(0, 0),
            ComposeStackStatus::Stopped
        );
    }

    #[test]
    fn destructive_actions_are_flagged_for_confirmation() {
        assert!(ComposeAction::Down.is_destructive());
        assert!(ComposeAction::Remove.is_destructive());
        assert!(!ComposeAction::Stop.is_destructive());
        assert!(!ComposeAction::Up.is_destructive());
    }

    #[test]
    fn only_file_driven_actions_require_files() {
        assert!(ComposeAction::Up.needs_files());
        assert!(ComposeAction::Build.needs_files());
        assert!(!ComposeAction::Stop.needs_files());
        assert!(!ComposeAction::Down.needs_files());
    }
}
