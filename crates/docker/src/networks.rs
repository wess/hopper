//! Network operations.

use crate::client::{Client, Req};
use crate::error::{DockerError, Result};
use model::{InspectResult, Ipam, Network, NetworkCreateInput, PruneReport};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

#[derive(Debug, Deserialize, Default)]
struct RawIpamConfig {
    #[serde(rename = "Subnet")]
    subnet: Option<String>,
    #[serde(rename = "Gateway")]
    gateway: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct RawIpam {
    #[serde(rename = "Config")]
    config: Option<Vec<RawIpamConfig>>,
}

#[derive(Debug, Deserialize)]
struct RawNetwork {
    #[serde(rename = "Id")]
    id: String,
    #[serde(rename = "Name")]
    name: Option<String>,
    #[serde(rename = "Driver")]
    driver: Option<String>,
    #[serde(rename = "Scope")]
    scope: Option<String>,
    #[serde(rename = "Internal")]
    internal: Option<bool>,
    #[serde(rename = "Attachable")]
    attachable: Option<bool>,
    #[serde(rename = "IPAM")]
    ipam: Option<RawIpam>,
    #[serde(rename = "Containers")]
    containers: Option<Map<String, Value>>,
    #[serde(rename = "Created")]
    created: Option<String>,
    #[serde(rename = "Labels")]
    labels: Option<BTreeMap<String, String>>,
}

fn map_network(n: RawNetwork) -> Network {
    Network {
        id: n.id,
        name: n.name.unwrap_or_default(),
        driver: n.driver.unwrap_or_default(),
        scope: n.scope.unwrap_or_default(),
        internal: n.internal.unwrap_or(false),
        attachable: n.attachable.unwrap_or(false),
        ipam: n
            .ipam
            .and_then(|i| i.config)
            .unwrap_or_default()
            .into_iter()
            .map(|c| Ipam {
                subnet: c.subnet,
                gateway: c.gateway,
            })
            .collect(),
        containers: n.containers.map(|c| c.len()).unwrap_or(0),
        created: n.created.unwrap_or_default(),
        labels: n.labels.unwrap_or_default(),
    }
}

pub async fn list(client: &Client) -> Result<Vec<Network>> {
    let raw: Vec<RawNetwork> = client.json(Req::get("/networks")).await?;
    Ok(raw.into_iter().map(map_network).collect())
}

pub async fn inspect(client: &Client, id: &str) -> Result<InspectResult> {
    client.json(Req::get(format!("/networks/{id}"))).await
}

/// The create body. Split out so the IPAM shaping can be asserted without a
/// daemon — Docker rejects an empty `Config` array differently from an absent
/// `IPAM` key.
pub fn create_body(input: &NetworkCreateInput) -> Value {
    let mut body = Map::new();
    body.insert("Name".into(), json!(input.name));
    body.insert(
        "Driver".into(),
        json!(input.driver.as_deref().unwrap_or("bridge")),
    );
    body.insert("Internal".into(), json!(input.internal));
    body.insert("Attachable".into(), json!(input.attachable));
    body.insert("CheckDuplicate".into(), json!(true));

    let subnet = input.subnet.as_deref().filter(|s| !s.trim().is_empty());
    let gateway = input.gateway.as_deref().filter(|s| !s.trim().is_empty());
    if subnet.is_some() || gateway.is_some() {
        let mut cfg = Map::new();
        if let Some(s) = subnet {
            cfg.insert("Subnet".into(), json!(s));
        }
        if let Some(g) = gateway {
            cfg.insert("Gateway".into(), json!(g));
        }
        body.insert("IPAM".into(), json!({ "Config": [Value::Object(cfg)] }));
    }
    Value::Object(body)
}

#[derive(Debug, Deserialize, Default)]
struct Created {
    #[serde(rename = "Id")]
    id: String,
}

pub async fn create(client: &Client, input: &NetworkCreateInput) -> Result<String> {
    let created: Created = client
        .json(Req::post("/networks/create").json_body(create_body(input)))
        .await?;
    Ok(created.id)
}

pub async fn remove(client: &Client, id: &str) -> Result<()> {
    client.action(Req::delete(format!("/networks/{id}"))).await
}

pub async fn connect(client: &Client, id: &str, container: &str) -> Result<()> {
    client
        .action(
            Req::post(format!("/networks/{id}/connect")).json_body(json!({ "Container": container })),
        )
        .await
}

pub async fn disconnect(client: &Client, id: &str, container: &str, force: bool) -> Result<()> {
    client
        .action(
            Req::post(format!("/networks/{id}/disconnect"))
                .json_body(json!({ "Container": container, "Force": force })),
        )
        .await
}

#[derive(Debug, Deserialize, Default)]
struct RawPrune {
    #[serde(rename = "NetworksDeleted")]
    deleted: Option<Vec<String>>,
}

pub async fn prune(client: &Client) -> Result<PruneReport> {
    let raw: RawPrune = client.json(Req::post("/networks/prune")).await?;
    Ok(PruneReport {
        kind: "networks".into(),
        removed: raw.deleted.unwrap_or_default().len() as i64,
        // Networks occupy no disk, so nothing is reclaimed by removing them.
        reclaimed: 0,
    })
}

/// Guard against removing one of Docker's own networks. The daemon would
/// refuse anyway, but with a less helpful message than this.
pub fn ensure_removable(net: &Network) -> Result<()> {
    if net.is_builtin() {
        return Err(DockerError::api(
            403,
            format!("{} is a built-in Docker network and cannot be removed.", net.name),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_a_network_with_ipam_and_counts_attached_containers() {
        let raw: RawNetwork = serde_json::from_value(json!({
            "Id": "net1",
            "Name": "shop_default",
            "Driver": "bridge",
            "Scope": "local",
            "Internal": false,
            "Attachable": true,
            "IPAM": {"Config": [{"Subnet": "172.20.0.0/16", "Gateway": "172.20.0.1"}]},
            "Containers": {"a": {}, "b": {}},
            "Created": "2026-01-01T00:00:00Z"
        }))
        .unwrap();
        let n = map_network(raw);
        assert_eq!(n.containers, 2);
        assert_eq!(n.ipam[0].subnet.as_deref(), Some("172.20.0.0/16"));
        assert!(n.attachable);
    }

    #[test]
    fn a_network_without_ipam_maps_to_an_empty_list() {
        let raw: RawNetwork = serde_json::from_value(json!({"Id": "n", "Name": "none"})).unwrap();
        let n = map_network(raw);
        assert!(n.ipam.is_empty());
        assert_eq!(n.containers, 0);
    }

    #[test]
    fn create_body_omits_ipam_entirely_when_no_subnet_is_given() {
        let input = NetworkCreateInput {
            name: "plain".into(),
            ..Default::default()
        };
        let body = create_body(&input);
        assert_eq!(body["Driver"], "bridge");
        assert!(body.get("IPAM").is_none());
    }

    #[test]
    fn create_body_includes_ipam_when_a_subnet_is_given() {
        let input = NetworkCreateInput {
            name: "custom".into(),
            subnet: Some("10.5.0.0/16".into()),
            gateway: Some("10.5.0.1".into()),
            driver: Some("bridge".into()),
            internal: true,
            attachable: true,
        };
        let body = create_body(&input);
        assert_eq!(body["IPAM"]["Config"][0]["Subnet"], "10.5.0.0/16");
        assert_eq!(body["IPAM"]["Config"][0]["Gateway"], "10.5.0.1");
        assert_eq!(body["Internal"], true);
    }

    #[test]
    fn blank_subnet_strings_do_not_produce_an_empty_ipam_block() {
        let input = NetworkCreateInput {
            name: "x".into(),
            subnet: Some("   ".into()),
            ..Default::default()
        };
        assert!(create_body(&input).get("IPAM").is_none());
    }

    #[test]
    fn builtin_networks_are_refused_before_the_daemon_is_asked() {
        let builtin = Network {
            name: "bridge".into(),
            ..Default::default()
        };
        let err = ensure_removable(&builtin).unwrap_err();
        assert!(err.message.contains("built-in"));

        let user = Network {
            name: "shop_default".into(),
            ..Default::default()
        };
        assert!(ensure_removable(&user).is_ok());
    }
}
