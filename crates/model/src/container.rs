//! Containers: the list projection, live stats frames, and run input.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Docker's lifecycle states, normalized. Drives the status dot and filters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContainerState {
    Created,
    Running,
    Paused,
    Restarting,
    Removing,
    Exited,
    Dead,
}

impl ContainerState {
    /// Parse the daemon's `State` string. Unknown values read as `Dead` rather
    /// than failing the whole list — one odd container must not blank the view.
    pub fn parse(raw: &str) -> Self {
        match raw {
            "created" => Self::Created,
            "running" => Self::Running,
            "paused" => Self::Paused,
            "restarting" => Self::Restarting,
            "removing" => Self::Removing,
            "exited" => Self::Exited,
            _ => Self::Dead,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Restarting => "restarting",
            Self::Removing => "removing",
            Self::Exited => "exited",
            Self::Dead => "dead",
        }
    }

    /// Whether lifecycle actions should treat this as live.
    pub fn is_up(&self) -> bool {
        matches!(self, Self::Running | Self::Restarting | Self::Paused)
    }
}

/// A container's health, when it declares a healthcheck.
///
/// The Bun build never modeled this: `/containers/json` folds health into the
/// human `status` string, so a `running` but `unhealthy` container drew a green
/// dot. Carrying it as its own field is what lets the UI tell those apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Health {
    None,
    Starting,
    Healthy,
    Unhealthy,
}

impl Health {
    /// Read health out of the daemon's human status string
    /// (`"Up 2 hours (healthy)"`). The list endpoint offers nothing better;
    /// `/containers/{id}/json` carries `State.Health.Status` properly and
    /// [`Health::parse_status`] handles that.
    pub fn from_status(status: &str) -> Self {
        if status.contains("(healthy)") {
            Self::Healthy
        } else if status.contains("(unhealthy)") {
            Self::Unhealthy
        } else if status.contains("(health: starting)") {
            Self::Starting
        } else {
            Self::None
        }
    }

    pub fn parse_status(raw: &str) -> Self {
        match raw {
            "healthy" => Self::Healthy,
            "unhealthy" => Self::Unhealthy,
            "starting" => Self::Starting,
            _ => Self::None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Starting => "starting",
            Self::Healthy => "healthy",
            Self::Unhealthy => "unhealthy",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Port {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub ip: Option<String>,
    pub private_port: u16,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub public_port: Option<u16>,
    /// tcp | udp | sctp
    #[serde(rename = "type")]
    pub proto: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mount {
    /// bind | volume | tmpfs
    #[serde(rename = "type")]
    pub kind: String,
    pub source: String,
    pub destination: String,
    pub mode: String,
    pub rw: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Container {
    pub id: String,
    /// Primary name, no leading slash.
    pub name: String,
    pub image: String,
    pub image_id: String,
    pub command: String,
    /// Unix seconds.
    pub created: i64,
    pub state: ContainerState,
    /// Human string, e.g. "Up 3 hours".
    pub status: String,
    pub health: Health,
    pub ports: Vec<Port>,
    pub labels: BTreeMap<String, String>,
    pub mounts: Vec<Mount>,
    pub networks: Vec<String>,
    /// The compose project/service this container belongs to, from the
    /// `com.docker.compose.*` labels. Drives stack grouping.
    pub compose_project: Option<String>,
    pub compose_service: Option<String>,
}

impl Container {
    /// The short id Docker itself displays.
    pub fn short_id(&self) -> &str {
        let end = self.id.len().min(12);
        &self.id[..end]
    }
}

/// One sampled frame from the stats stream, already reduced to display values.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerStats {
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub id: String,
    pub cpu_percent: f64,
    /// Bytes.
    pub mem_usage: u64,
    pub mem_limit: u64,
    pub mem_percent: f64,
    pub net_rx: u64,
    pub net_tx: u64,
    pub block_read: u64,
    pub block_write: u64,
    pub pids: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessList {
    pub titles: Vec<String>,
    pub processes: Vec<Vec<String>>,
}

/// A published port in [`RunInput`]. Strings because the run dialog collects
/// raw text; parsing and validation happen when the create body is built.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortMapping {
    pub host: String,
    pub container: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub proto: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeMapping {
    pub host: String,
    pub container: String,
    #[serde(default)]
    pub ro: bool,
}

/// Resource limits applied at create time.
///
/// Docker Desktop exposes these in its run dialog and Hopper's Bun build did
/// not, so a container that needed a memory cap had to be created from the CLI.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceLimits {
    /// Whole CPUs, as `--cpus`. Converted to `NanoCpus` on the wire.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub cpus: Option<f64>,
    /// Hard memory limit in bytes.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub memory: Option<u64>,
    /// Soft limit; must not exceed `memory`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub memory_reservation: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub pids_limit: Option<i64>,
}

impl ResourceLimits {
    pub fn is_empty(&self) -> bool {
        self.cpus.is_none()
            && self.memory.is_none()
            && self.memory_reservation.is_none()
            && self.pids_limit.is_none()
    }
}

/// Input for creating and starting a container (the "Run" dialog).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunInput {
    pub image: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub name: Option<String>,
    /// `KEY=VALUE` pairs.
    #[serde(default)]
    pub env: Vec<String>,
    #[serde(default)]
    pub ports: Vec<PortMapping>,
    #[serde(default)]
    pub volumes: Vec<VolumeMapping>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub command: Option<String>,
    /// no | always | unless-stopped | on-failure
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub restart: Option<String>,
    #[serde(default)]
    pub auto_remove: bool,
    /// Attach to this network at create time instead of the default bridge.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub network: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub workdir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub hostname: Option<String>,
    #[serde(default)]
    pub limits: ResourceLimits,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    /// Run with a TTY attached, as `-t`.
    #[serde(default)]
    pub tty: bool,
}

/// A live update to a container's resources or restart policy, applied through
/// `POST /containers/{id}/update` without recreating it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInput {
    #[serde(default)]
    pub limits: ResourceLimits,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub restart: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_parses_known_values_and_falls_back_to_dead() {
        assert_eq!(ContainerState::parse("running"), ContainerState::Running);
        assert_eq!(ContainerState::parse("paused"), ContainerState::Paused);
        assert_eq!(ContainerState::parse("nonsense"), ContainerState::Dead);
    }

    #[test]
    fn health_reads_out_of_the_daemons_status_string() {
        assert_eq!(Health::from_status("Up 2 hours (healthy)"), Health::Healthy);
        assert_eq!(
            Health::from_status("Up 5 seconds (health: starting)"),
            Health::Starting
        );
        assert_eq!(
            Health::from_status("Up 3 days (unhealthy)"),
            Health::Unhealthy
        );
        assert_eq!(Health::from_status("Up 3 days"), Health::None);
    }

    #[test]
    fn short_id_never_panics_on_a_short_id() {
        let c = Container {
            id: "abc".into(),
            name: "x".into(),
            image: String::new(),
            image_id: String::new(),
            command: String::new(),
            created: 0,
            state: ContainerState::Running,
            status: String::new(),
            health: Health::None,
            ports: vec![],
            labels: BTreeMap::new(),
            mounts: vec![],
            networks: vec![],
            compose_project: None,
            compose_service: None,
        };
        assert_eq!(c.short_id(), "abc");
    }
}
