//! Docker Desktop → Hopper migration.

use serde::{Deserialize, Serialize};

/// A serializable Docker connection target.
///
/// The scan resolves the source engine and the plan pins it, so a daemon coming
/// up or down between scan and run cannot silently redirect the migration to a
/// different engine.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "transport", rename_all = "camelCase")]
pub enum MigrationEndpoint {
    #[serde(rename = "unix")]
    Unix { path: String },
    #[serde(rename = "npipe")]
    Npipe { path: String },
    #[serde(rename = "tcp")]
    Tcp { host: String, port: u16, tls: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MigrationKind {
    Image,
    Volume,
    Network,
    Container,
}

impl MigrationKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Volume => "volume",
            Self::Network => "network",
            Self::Container => "container",
        }
    }
}

/// One migratable object discovered on the source engine.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationItem {
    pub kind: MigrationKind,
    /// Image id / volume name / network id / container id.
    pub id: String,
    /// repo:tag / volume name / network name / container name.
    pub name: String,
    /// Size, status, or backing image.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub detail: Option<String>,
}

/// What the source engine holds, surfaced for selection.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationScan {
    /// A migratable source engine was found.
    pub available: bool,
    /// Human description of the source endpoint.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub source: Option<String>,
    /// The resolved source, pinned into the plan.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub source_endpoint: Option<MigrationEndpoint>,
    pub containers: Vec<MigrationItem>,
    pub images: Vec<MigrationItem>,
    pub volumes: Vec<MigrationItem>,
    pub networks: Vec<MigrationItem>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub message: Option<String>,
}

/// The user's selection of what to migrate, plus the pinned source.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationPlan {
    /// The exact engine the selection came from.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub source: Option<MigrationEndpoint>,
    pub containers: Vec<String>,
    pub images: Vec<String>,
    pub volumes: Vec<String>,
    pub networks: Vec<String>,
}

impl MigrationPlan {
    pub fn is_empty(&self) -> bool {
        self.containers.is_empty()
            && self.images.is_empty()
            && self.volumes.is_empty()
            && self.networks.is_empty()
    }

    pub fn total(&self) -> usize {
        self.containers.len() + self.images.len() + self.volumes.len() + self.networks.len()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MigrationPhase {
    Networks,
    Volumes,
    Images,
    Containers,
    Done,
    Error,
}

impl MigrationPhase {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Networks => "networks",
            Self::Volumes => "volumes",
            Self::Images => "images",
            Self::Containers => "containers",
            Self::Done => "done",
            Self::Error => "error",
        }
    }
}

/// Streamed migration progress.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationProgress {
    pub phase: MigrationPhase,
    /// The current object's name; empty between items.
    pub item: String,
    /// Items completed in this phase.
    pub done: usize,
    /// Items in this phase.
    pub total: usize,
    pub message: String,
    /// Non-fatal per-item error; the migration continues.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub error: Option<String>,
    /// Non-fatal advisory (host-path bind, arch mismatch, …).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub warning: Option<String>,
    /// The whole migration is complete.
    #[serde(default)]
    pub finished: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_round_trips_through_json_with_the_transport_tag() {
        let ep = MigrationEndpoint::Unix {
            path: "/var/run/docker.sock".into(),
        };
        let raw = serde_json::to_string(&ep).unwrap();
        assert!(raw.contains(r#""transport":"unix""#));
        let back: MigrationEndpoint = serde_json::from_str(&raw).unwrap();
        assert_eq!(back, ep);
    }

    #[test]
    fn tcp_endpoint_round_trips() {
        let ep = MigrationEndpoint::Tcp {
            host: "example.test".into(),
            port: 2376,
            tls: true,
        };
        let raw = serde_json::to_string(&ep).unwrap();
        let back: MigrationEndpoint = serde_json::from_str(&raw).unwrap();
        assert_eq!(back, ep);
    }

    #[test]
    fn plan_totals_across_kinds() {
        let plan = MigrationPlan {
            source: None,
            containers: vec!["a".into()],
            images: vec!["b".into(), "c".into()],
            volumes: vec![],
            networks: vec!["d".into()],
        };
        assert!(!plan.is_empty());
        assert_eq!(plan.total(), 4);
        assert!(MigrationPlan::default().is_empty());
    }
}
