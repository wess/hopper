//! Apple's JSON, and how it becomes Hopper's model.
//!
//! Deliberately liberal: `container` promises stability only within a patch
//! version, so every field is optional and anything structural we do not need
//! stays a `Value`. A field Apple renames must cost one column in a list, not
//! the whole view.

use std::collections::BTreeMap;

use model::{Container, ContainerState, Health, Image, Mount, Network, Port, Volume};
use serde::Deserialize;

// --- containers ----------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedContainer {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub configuration: ContainerConfiguration,
    #[serde(default)]
    pub status: ContainerStatus,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerConfiguration {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub image: ImageDescription,
    #[serde(default)]
    pub mounts: Vec<Filesystem>,
    #[serde(default)]
    pub published_ports: Vec<PublishPort>,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    #[serde(default)]
    pub init_process: InitProcess,
    #[serde(default)]
    pub creation_date: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageDescription {
    #[serde(default)]
    pub reference: String,
    #[serde(default)]
    pub descriptor: Descriptor,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Descriptor {
    #[serde(default)]
    pub digest: String,
    #[serde(default)]
    pub size: i64,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitProcess {
    #[serde(default)]
    pub executable: String,
    #[serde(default)]
    pub arguments: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerStatus {
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub networks: Vec<Attachment>,
    #[serde(default)]
    pub started_date: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    #[serde(default)]
    pub network: String,
    #[serde(default)]
    pub hostname: String,
    #[serde(default)]
    pub ipv4_address: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishPort {
    #[serde(default)]
    pub host_address: Option<String>,
    #[serde(default)]
    pub host_port: u16,
    #[serde(default)]
    pub container_port: u16,
    #[serde(default, rename = "proto")]
    pub proto: Option<String>,
    #[serde(default)]
    pub count: Option<u16>,
}

/// A mount. `type` is a Swift enum with associated values, so it arrives as a
/// single-key object — `{"volume":{"name":"data",…}}` — rather than a string.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Filesystem {
    #[serde(default, rename = "type")]
    pub kind: serde_json::Value,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub destination: String,
    #[serde(default)]
    pub options: serde_json::Value,
}

impl Filesystem {
    /// The variant name of the `type` object, e.g. `volume`, `virtiofs`.
    fn variant(&self) -> Option<&str> {
        match &self.kind {
            serde_json::Value::Object(m) => m.keys().next().map(String::as_str),
            serde_json::Value::String(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// The named volume backing this mount, when there is one.
    fn volume_name(&self) -> Option<String> {
        let serde_json::Value::Object(m) = &self.kind else {
            return None;
        };
        m.get("volume")?
            .get("name")?
            .as_str()
            .map(str::to_string)
    }

    fn readonly(&self) -> bool {
        let text = self.options.to_string().to_lowercase();
        text.contains("\"ro\"") || text.contains("readonly")
    }
}

// --- images / volumes / networks -----------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageResource {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub configuration: ImageConfiguration,
    #[serde(default)]
    pub variants: Vec<ImageVariant>,
    #[serde(default)]
    pub display_reference: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageConfiguration {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub creation_date: Option<String>,
    #[serde(default)]
    pub descriptor: Descriptor,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageVariant {
    #[serde(default)]
    pub digest: String,
    #[serde(default)]
    pub size: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeResource {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub configuration: VolumeConfiguration,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeConfiguration {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    #[serde(default)]
    pub creation_date: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub size_in_bytes: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkResource {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub configuration: NetworkConfiguration,
    #[serde(default)]
    pub status: serde_json::Value,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkConfiguration {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    #[serde(default)]
    pub creation_date: Option<String>,
    #[serde(default)]
    pub subnet: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
}

// --- conversion ----------------------------------------------------------

/// ISO8601 to unix seconds. Apple renders dates through `.iso8601`, but a
/// missing or odd value must not cost us the row.
pub fn seconds(iso: Option<&String>) -> i64 {
    iso.and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.timestamp())
        .unwrap_or(0)
}

/// Apple's `RuntimeStatus` in Hopper's terms.
///
/// `stopping` maps to `Restarting` because that is the one state Hopper treats
/// as *in flux but still up* — the container is going down, so offering
/// "Start" would be wrong, and so would a green dot.
pub fn state(raw: &str) -> ContainerState {
    match raw {
        "running" => ContainerState::Running,
        "stopped" => ContainerState::Exited,
        "stopping" => ContainerState::Restarting,
        _ => ContainerState::Dead,
    }
}

/// Strip the implied registry so a list reads `nginx:latest` rather than
/// `docker.io/library/nginx:latest`, matching what `container image ls` shows.
pub fn short_reference(reference: &str) -> String {
    let r = reference
        .strip_prefix("docker.io/library/")
        .or_else(|| reference.strip_prefix("docker.io/"))
        .unwrap_or(reference);
    r.to_string()
}

impl ManagedContainer {
    pub fn into_model(self) -> Container {
        let cfg = self.configuration;
        // Apple containers carry no separate name: the id the user chose with
        // `--name` *is* the id.
        let id = if self.id.is_empty() { cfg.id.clone() } else { self.id };

        let mut command = cfg.init_process.executable.clone();
        for arg in &cfg.init_process.arguments {
            command.push(' ');
            command.push_str(arg);
        }

        let state = state(&cfg_state(&self.status));
        let started = seconds(self.status.started_date.as_ref());
        let status = match state {
            ContainerState::Running if started > 0 => "Up".to_string(),
            ContainerState::Running => "Up".to_string(),
            ContainerState::Restarting => "Stopping".to_string(),
            ContainerState::Exited => "Exited".to_string(),
            _ => "Unknown".to_string(),
        };

        let mut ports = Vec::new();
        for p in &cfg.published_ports {
            // `count` publishes a contiguous range from one entry.
            for offset in 0..p.count.unwrap_or(1).max(1) {
                ports.push(Port {
                    ip: p.host_address.clone(),
                    private_port: p.container_port.saturating_add(offset),
                    public_port: Some(p.host_port.saturating_add(offset)),
                    proto: p.proto.clone().unwrap_or_else(|| "tcp".into()),
                });
            }
        }

        let mounts = cfg
            .mounts
            .iter()
            .map(|m| {
                let name = m.volume_name();
                let kind = match m.variant() {
                    Some("volume") | Some("block") => "volume",
                    Some("tmpfs") => "tmpfs",
                    _ => "bind",
                };
                Mount {
                    kind: kind.to_string(),
                    source: m.source.clone(),
                    destination: m.destination.clone(),
                    mode: if m.readonly() { "ro".into() } else { "rw".into() },
                    rw: !m.readonly(),
                    name,
                }
            })
            .collect();

        let networks: Vec<String> = self
            .status
            .networks
            .iter()
            .map(|n| n.network.clone())
            .filter(|n| !n.is_empty())
            .collect();

        let compose_project = cfg.labels.get("com.docker.compose.project").cloned();
        let compose_service = cfg.labels.get("com.docker.compose.service").cloned();

        Container {
            id,
            name: cfg.id,
            image: short_reference(&cfg.image.reference),
            image_id: cfg.image.descriptor.digest,
            command,
            created: seconds(cfg.creation_date.as_ref()),
            state,
            status,
            // Apple's runtime has no healthcheck concept, so every container
            // reports "none" rather than a green tick it never earned.
            health: Health::None,
            ports,
            labels: cfg.labels,
            mounts,
            networks,
            compose_project,
            compose_service,
        }
    }
}

fn cfg_state(status: &ContainerStatus) -> String {
    status.state.clone()
}

impl ImageResource {
    pub fn into_model(self) -> Image {
        let cfg = self.configuration;
        let reference = self
            .display_reference
            .clone()
            .unwrap_or_else(|| short_reference(&cfg.name));
        let size = if cfg.descriptor.size > 0 {
            cfg.descriptor.size
        } else {
            self.variants.iter().map(|v| v.size).max().unwrap_or(0)
        };
        let id = if self.id.is_empty() {
            cfg.descriptor.digest.clone()
        } else {
            self.id.clone()
        };
        Image {
            id,
            repo_tags: vec![reference],
            repo_digests: self.variants.iter().map(|v| v.digest.clone()).collect(),
            created: seconds(cfg.creation_date.as_ref()),
            size,
            // Apple reports no per-image container count; -1 is the model's
            // "unknown" rather than a claim that nothing uses it.
            containers: -1,
            dangling: false,
            labels: BTreeMap::new(),
        }
    }
}

impl VolumeResource {
    pub fn into_model(self) -> Volume {
        let cfg = self.configuration;
        let name = if cfg.name.is_empty() { self.id } else { cfg.name };
        Volume {
            name,
            driver: cfg.format.unwrap_or_else(|| "local".into()),
            mountpoint: cfg.source.unwrap_or_default(),
            created: cfg.creation_date.unwrap_or_default(),
            scope: "local".into(),
            labels: cfg.labels,
            size: cfg.size_in_bytes.unwrap_or(-1),
            // Establishing use means cross-referencing every container; the
            // volumes view fills this in when it has the container list.
            in_use: false,
        }
    }
}

impl NetworkResource {
    pub fn into_model(self) -> Network {
        let cfg = self.configuration;
        let name = if cfg.name.is_empty() { self.id.clone() } else { cfg.name };
        let subnet = cfg
            .subnet
            .or_else(|| self.status.get("address")?.as_str().map(str::to_string));
        Network {
            id: name.clone(),
            name,
            driver: cfg.mode.unwrap_or_else(|| "vmnet".into()),
            scope: "local".into(),
            internal: false,
            attachable: true,
            ipam: subnet
                .into_iter()
                .map(|subnet| model::Ipam {
                    subnet: Some(subnet),
                    gateway: None,
                })
                .collect(),
            containers: 0,
            created: cfg.creation_date.unwrap_or_default(),
            labels: cfg.labels,
        }
    }
}
