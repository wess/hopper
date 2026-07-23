//! Container operations, mapping raw Engine API responses into the lean
//! [`Container`] shape the UI consumes.

use crate::client::{Client, Req};
use crate::error::Result;
use model::{
    Container, ContainerState, Health, InspectResult, Mount, Port, ProcessList, PruneReport,
    ResourceLimits, RunInput, UpdateInput,
};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

#[derive(Debug, Deserialize)]
struct RawPort {
    #[serde(rename = "IP")]
    ip: Option<String>,
    #[serde(rename = "PrivatePort")]
    private_port: u16,
    #[serde(rename = "PublicPort")]
    public_port: Option<u16>,
    #[serde(rename = "Type")]
    kind: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawMount {
    #[serde(rename = "Type")]
    kind: Option<String>,
    #[serde(rename = "Source")]
    source: Option<String>,
    #[serde(rename = "Destination")]
    destination: Option<String>,
    #[serde(rename = "Mode")]
    mode: Option<String>,
    #[serde(rename = "RW")]
    rw: Option<bool>,
    #[serde(rename = "Name")]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawNetworkSettings {
    #[serde(rename = "Networks")]
    networks: Option<Map<String, Value>>,
}

#[derive(Debug, Deserialize)]
struct RawContainer {
    #[serde(rename = "Id")]
    id: String,
    #[serde(rename = "Names")]
    names: Option<Vec<String>>,
    #[serde(rename = "Image")]
    image: Option<String>,
    #[serde(rename = "ImageID")]
    image_id: Option<String>,
    #[serde(rename = "Command")]
    command: Option<String>,
    #[serde(rename = "Created")]
    created: Option<i64>,
    #[serde(rename = "State")]
    state: Option<String>,
    #[serde(rename = "Status")]
    status: Option<String>,
    #[serde(rename = "Ports")]
    ports: Option<Vec<RawPort>>,
    #[serde(rename = "Labels")]
    labels: Option<BTreeMap<String, String>>,
    #[serde(rename = "Mounts")]
    mounts: Option<Vec<RawMount>>,
    #[serde(rename = "NetworkSettings")]
    network_settings: Option<RawNetworkSettings>,
}

fn map_container(c: RawContainer) -> Container {
    let labels = c.labels.unwrap_or_default();
    let name = c
        .names
        .and_then(|n| n.into_iter().next())
        .unwrap_or_default()
        .trim_start_matches('/')
        .to_string();
    let status = c.status.unwrap_or_default();
    Container {
        id: c.id,
        name,
        image: c.image.unwrap_or_default(),
        image_id: c.image_id.unwrap_or_default(),
        command: c.command.unwrap_or_default(),
        created: c.created.unwrap_or_default(),
        state: ContainerState::parse(&c.state.unwrap_or_default().to_lowercase()),
        health: Health::from_status(&status),
        status,
        ports: c
            .ports
            .unwrap_or_default()
            .into_iter()
            .map(|p| Port {
                ip: p.ip,
                private_port: p.private_port,
                public_port: p.public_port,
                proto: p.kind.unwrap_or_else(|| "tcp".into()),
            })
            .collect(),
        mounts: c
            .mounts
            .unwrap_or_default()
            .into_iter()
            .map(|m| Mount {
                kind: m.kind.unwrap_or_default(),
                source: m.source.unwrap_or_default(),
                destination: m.destination.unwrap_or_default(),
                mode: m.mode.unwrap_or_default(),
                rw: m.rw.unwrap_or(true),
                name: m.name,
            })
            .collect(),
        networks: c
            .network_settings
            .and_then(|n| n.networks)
            .map(|n| n.keys().cloned().collect())
            .unwrap_or_default(),
        compose_project: labels.get("com.docker.compose.project").cloned(),
        compose_service: labels.get("com.docker.compose.service").cloned(),
        labels,
    }
}

pub async fn list(client: &Client, all: bool) -> Result<Vec<Container>> {
    let raw: Vec<RawContainer> = client
        .json(Req::get("/containers/json").flag("all", all))
        .await?;
    Ok(raw.into_iter().map(map_container).collect())
}

pub async fn inspect(client: &Client, id: &str) -> Result<InspectResult> {
    client.json(Req::get(format!("/containers/{id}/json"))).await
}

/// Health as the daemon reports it on inspect, which is authoritative —
/// unlike the list endpoint, which only folds it into the status string.
pub async fn health(client: &Client, id: &str) -> Result<Health> {
    let raw: Value = inspect(client, id).await?;
    Ok(raw
        .pointer("/State/Health/Status")
        .and_then(|v| v.as_str())
        .map(Health::parse_status)
        .unwrap_or(Health::None))
}

/// Whether the container was created with a TTY. The log and exec readers need
/// this to know whether output is stdcopy-framed.
pub async fn has_tty(client: &Client, id: &str) -> Result<bool> {
    let raw: Value = inspect(client, id).await?;
    Ok(raw
        .pointer("/Config/Tty")
        .and_then(|v| v.as_bool())
        .unwrap_or(false))
}

pub async fn start(client: &Client, id: &str) -> Result<()> {
    client
        .idempotent(Req::post(format!("/containers/{id}/start")))
        .await
}

pub async fn stop(client: &Client, id: &str) -> Result<()> {
    client
        .idempotent(Req::post(format!("/containers/{id}/stop")).query("t", 10))
        .await
}

pub async fn restart(client: &Client, id: &str) -> Result<()> {
    client
        .action(Req::post(format!("/containers/{id}/restart")).query("t", 10))
        .await
}

pub async fn pause(client: &Client, id: &str) -> Result<()> {
    client
        .action(Req::post(format!("/containers/{id}/pause")))
        .await
}

pub async fn unpause(client: &Client, id: &str) -> Result<()> {
    client
        .action(Req::post(format!("/containers/{id}/unpause")))
        .await
}

pub async fn kill(client: &Client, id: &str) -> Result<()> {
    client
        .action(Req::post(format!("/containers/{id}/kill")))
        .await
}

pub async fn rename(client: &Client, id: &str, name: &str) -> Result<()> {
    client
        .action(Req::post(format!("/containers/{id}/rename")).query("name", name))
        .await
}

pub async fn remove(client: &Client, id: &str, force: bool, volumes: bool) -> Result<()> {
    client
        .action(
            Req::delete(format!("/containers/{id}"))
                .flag("force", force)
                .flag("v", volumes),
        )
        .await
}

#[derive(Debug, Deserialize, Default)]
struct RawTop {
    #[serde(rename = "Titles")]
    titles: Option<Vec<String>>,
    #[serde(rename = "Processes")]
    processes: Option<Vec<Vec<String>>>,
}

pub async fn top(client: &Client, id: &str) -> Result<ProcessList> {
    let raw: RawTop = client
        .json(Req::get(format!("/containers/{id}/top")).query("ps_args", "aux"))
        .await?;
    Ok(ProcessList {
        titles: raw.titles.unwrap_or_default(),
        processes: raw.processes.unwrap_or_default(),
    })
}

#[derive(Debug, Deserialize, Default)]
struct RawPrune {
    #[serde(rename = "ContainersDeleted")]
    deleted: Option<Vec<String>>,
    #[serde(rename = "SpaceReclaimed")]
    reclaimed: Option<i64>,
}

pub async fn prune(client: &Client) -> Result<PruneReport> {
    let raw: RawPrune = client.json(Req::post("/containers/prune")).await?;
    Ok(PruneReport {
        kind: "containers".into(),
        removed: raw.deleted.unwrap_or_default().len() as i64,
        reclaimed: raw.reclaimed.unwrap_or_default(),
    })
}

/// Split a command string the way a shell would, honoring quotes.
///
/// The Bun build split on whitespace, so `sh -c "echo hi"` became four
/// arguments and the container failed to start.
pub fn split_command(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut started = false;

    for ch in input.chars() {
        if escaped {
            cur.push(ch);
            escaped = false;
            continue;
        }
        match (quote, ch) {
            (None, '\\') => escaped = true,
            (Some('"'), '\\') => escaped = true,
            (None, '\'') | (None, '"') => {
                quote = Some(ch);
                started = true;
            }
            (Some(q), c) if c == q => {
                quote = None;
            }
            (None, c) if c.is_whitespace() => {
                if started || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            (_, c) => {
                cur.push(c);
                started = true;
            }
        }
    }
    if started || !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Build the `HostConfig` fragment shared by create and update.
fn host_config_limits(limits: &ResourceLimits) -> Map<String, Value> {
    let mut m = Map::new();
    if let Some(cpus) = limits.cpus.filter(|c| *c > 0.0) {
        // Docker expresses `--cpus` as billionths of a CPU.
        m.insert(
            "NanoCpus".into(),
            json!((cpus * 1_000_000_000.0).round() as i64),
        );
    }
    if let Some(mem) = limits.memory.filter(|m| *m > 0) {
        m.insert("Memory".into(), json!(mem));
    }
    if let Some(res) = limits.memory_reservation.filter(|m| *m > 0) {
        m.insert("MemoryReservation".into(), json!(res));
    }
    if let Some(pids) = limits.pids_limit.filter(|p| *p > 0) {
        m.insert("PidsLimit".into(), json!(pids));
    }
    m
}

/// The create body for a run request. Split out from [`run`] so it can be
/// asserted on without a daemon.
pub fn create_body(input: &RunInput) -> Value {
    let mut exposed = Map::new();
    let mut bindings = Map::new();
    for p in &input.ports {
        let proto = p.proto.as_deref().unwrap_or("tcp");
        let key = format!("{}/{}", p.container, proto);
        exposed.insert(key.clone(), json!({}));
        let mut binding = Map::new();
        // An empty host port asks Docker to pick a free one, which is what the
        // dialog's blank field should mean.
        binding.insert("HostPort".into(), json!(p.host));
        bindings.insert(key, json!([binding]));
    }

    let binds: Vec<String> = input
        .volumes
        .iter()
        .map(|v| {
            let ro = if v.ro { ":ro" } else { "" };
            format!("{}:{}{}", v.host, v.container, ro)
        })
        .collect();

    let mut host_config = host_config_limits(&input.limits);
    host_config.insert("PortBindings".into(), Value::Object(bindings));
    host_config.insert("Binds".into(), json!(binds));
    host_config.insert("AutoRemove".into(), json!(input.auto_remove));
    if let Some(restart) = input.restart.as_deref().filter(|r| !r.is_empty() && *r != "no") {
        host_config.insert("RestartPolicy".into(), json!({ "Name": restart }));
    }

    let mut body = Map::new();
    body.insert("Image".into(), json!(input.image));
    body.insert("Env".into(), json!(input.env));
    body.insert("ExposedPorts".into(), Value::Object(exposed));
    body.insert("HostConfig".into(), Value::Object(host_config));
    body.insert("Tty".into(), json!(input.tty));
    if !input.labels.is_empty() {
        body.insert("Labels".into(), json!(input.labels));
    }
    if let Some(cmd) = input.command.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
        body.insert("Cmd".into(), json!(split_command(cmd)));
    }
    for (key, value) in [
        ("WorkingDir", &input.workdir),
        ("User", &input.user),
        ("Hostname", &input.hostname),
    ] {
        if let Some(v) = value.as_deref().filter(|s| !s.trim().is_empty()) {
            body.insert(key.into(), json!(v));
        }
    }
    if let Some(net) = input.network.as_deref().filter(|s| !s.trim().is_empty()) {
        body.insert(
            "NetworkingConfig".into(),
            json!({ "EndpointsConfig": { net: {} } }),
        );
    }
    Value::Object(body)
}

#[derive(Debug, Deserialize, Default)]
struct Created {
    #[serde(rename = "Id")]
    id: String,
}

/// Create and start a container from the Run dialog input.
pub async fn run(client: &Client, input: &RunInput) -> Result<String> {
    let created: Created = client
        .json(
            Req::post("/containers/create")
                .query_opt("name", input.name.as_deref().filter(|n| !n.trim().is_empty()))
                .json_body(create_body(input)),
        )
        .await?;
    start(client, &created.id).await?;
    Ok(created.id)
}

/// Apply new resource limits or a restart policy to an existing container.
pub async fn update(client: &Client, id: &str, input: &UpdateInput) -> Result<()> {
    let mut body = host_config_limits(&input.limits);
    if let Some(restart) = input.restart.as_deref().filter(|r| !r.is_empty()) {
        body.insert("RestartPolicy".into(), json!({ "Name": restart }));
    }
    client
        .action(Req::post(format!("/containers/{id}/update")).json_body(Value::Object(body)))
        .await
}

/// Snapshot a container into a new image.
pub async fn commit(client: &Client, id: &str, repo: &str, tag: Option<&str>) -> Result<String> {
    let created: Created = client
        .json(
            Req::post("/commit")
                .query("container", id)
                .query("repo", repo)
                .query_opt("tag", tag),
        )
        .await?;
    Ok(created.id)
}

pub async fn exists(client: &Client, id: &str) -> bool {
    client
        .send(Req::get(format!("/containers/{id}/json")))
        .await
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::{PortMapping, VolumeMapping};

    #[test]
    fn maps_a_raw_container_into_the_lean_shape() {
        let raw: RawContainer = serde_json::from_value(json!({
            "Id": "abc123",
            "Names": ["/web"],
            "Image": "nginx:latest",
            "ImageID": "sha256:deadbeef",
            "Command": "nginx -g daemon off;",
            "Created": 1700000000i64,
            "State": "running",
            "Status": "Up 2 hours (healthy)",
            "Ports": [{"IP": "0.0.0.0", "PrivatePort": 80, "PublicPort": 8080, "Type": "tcp"}],
            "Labels": {
                "com.docker.compose.project": "shop",
                "com.docker.compose.service": "web"
            },
            "Mounts": [{
                "Type": "volume", "Source": "/var/lib/docker/volumes/data",
                "Destination": "/data", "Mode": "z", "RW": true, "Name": "data"
            }],
            "NetworkSettings": {"Networks": {"bridge": {}, "shop_default": {}}}
        }))
        .unwrap();

        let c = map_container(raw);
        assert_eq!(c.name, "web");
        assert_eq!(c.state, ContainerState::Running);
        assert_eq!(c.health, Health::Healthy);
        assert_eq!(c.compose_project.as_deref(), Some("shop"));
        assert_eq!(c.compose_service.as_deref(), Some("web"));
        assert_eq!(c.ports[0].public_port, Some(8080));
        assert_eq!(c.mounts[0].destination, "/data");
        assert_eq!(c.networks.len(), 2);
    }

    #[test]
    fn maps_a_sparse_container_without_panicking() {
        // Older daemons and odd states omit most fields.
        let raw: RawContainer = serde_json::from_value(json!({"Id": "x"})).unwrap();
        let c = map_container(raw);
        assert_eq!(c.id, "x");
        assert_eq!(c.name, "");
        assert_eq!(c.state, ContainerState::Dead);
        assert!(c.ports.is_empty());
        assert!(c.compose_project.is_none());
    }

    #[test]
    fn command_splitting_honors_quotes() {
        assert_eq!(split_command("nginx -g daemon"), ["nginx", "-g", "daemon"]);
        assert_eq!(
            split_command(r#"sh -c "echo hello world""#),
            ["sh", "-c", "echo hello world"]
        );
        assert_eq!(
            split_command("sh -c 'echo hi there'"),
            ["sh", "-c", "echo hi there"]
        );
    }

    #[test]
    fn command_splitting_collapses_extra_whitespace() {
        assert_eq!(split_command("  a   b  "), ["a", "b"]);
        assert!(split_command("   ").is_empty());
        assert!(split_command("").is_empty());
    }

    #[test]
    fn command_splitting_keeps_deliberately_empty_arguments() {
        assert_eq!(split_command(r#"prog "" x"#), ["prog", "", "x"]);
    }

    #[test]
    fn command_splitting_handles_escapes() {
        assert_eq!(split_command(r"echo a\ b"), ["echo", "a b"]);
    }

    #[test]
    fn create_body_maps_ports_volumes_and_restart() {
        let input = RunInput {
            image: "nginx".into(),
            ports: vec![PortMapping {
                host: "8080".into(),
                container: "80".into(),
                proto: None,
            }],
            volumes: vec![VolumeMapping {
                host: "/srv".into(),
                container: "/usr/share/nginx/html".into(),
                ro: true,
            }],
            restart: Some("always".into()),
            ..Default::default()
        };
        let body = create_body(&input);
        assert_eq!(body["Image"], "nginx");
        assert!(body["ExposedPorts"].get("80/tcp").is_some());
        assert_eq!(body["HostConfig"]["PortBindings"]["80/tcp"][0]["HostPort"], "8080");
        assert_eq!(
            body["HostConfig"]["Binds"][0],
            "/srv:/usr/share/nginx/html:ro"
        );
        assert_eq!(body["HostConfig"]["RestartPolicy"]["Name"], "always");
    }

    #[test]
    fn a_restart_policy_of_no_is_omitted_rather_than_sent() {
        let input = RunInput {
            image: "x".into(),
            restart: Some("no".into()),
            ..Default::default()
        };
        assert!(create_body(&input)["HostConfig"]
            .get("RestartPolicy")
            .is_none());
    }

    #[test]
    fn cpu_limits_convert_to_nanocpus() {
        let input = RunInput {
            image: "x".into(),
            limits: ResourceLimits {
                cpus: Some(1.5),
                memory: Some(512 * 1024 * 1024),
                ..Default::default()
            },
            ..Default::default()
        };
        let body = create_body(&input);
        assert_eq!(body["HostConfig"]["NanoCpus"], 1_500_000_000i64);
        assert_eq!(body["HostConfig"]["Memory"], 536_870_912i64);
    }

    #[test]
    fn zero_and_negative_limits_are_dropped_not_sent_as_unlimited() {
        let limits = ResourceLimits {
            cpus: Some(0.0),
            memory: Some(0),
            pids_limit: Some(-1),
            ..Default::default()
        };
        assert!(host_config_limits(&limits).is_empty());
    }

    #[test]
    fn a_network_choice_becomes_an_endpoint_config() {
        let input = RunInput {
            image: "x".into(),
            network: Some("shop_default".into()),
            ..Default::default()
        };
        let body = create_body(&input);
        assert!(body["NetworkingConfig"]["EndpointsConfig"]
            .get("shop_default")
            .is_some());
    }

    #[test]
    fn a_blank_command_is_omitted_so_the_image_default_runs() {
        let input = RunInput {
            image: "x".into(),
            command: Some("   ".into()),
            ..Default::default()
        };
        assert!(create_body(&input).get("Cmd").is_none());
    }
}
