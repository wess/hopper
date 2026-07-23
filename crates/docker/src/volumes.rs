//! Volume operations.

use crate::client::{Client, Req};
use crate::error::Result;
use model::{InspectResult, PruneReport, Volume};
use serde::Deserialize;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Deserialize, Default)]
struct RawUsage {
    #[serde(rename = "Size")]
    size: Option<i64>,
}

#[derive(Debug, Default, Deserialize)]
struct RawVolume {
    #[serde(rename = "Name")]
    #[serde(default)]
    name: String,
    #[serde(rename = "Driver")]
    driver: Option<String>,
    #[serde(rename = "Mountpoint")]
    mountpoint: Option<String>,
    #[serde(rename = "CreatedAt")]
    created_at: Option<String>,
    #[serde(rename = "Scope")]
    scope: Option<String>,
    #[serde(rename = "Labels")]
    labels: Option<BTreeMap<String, String>>,
    #[serde(rename = "UsageData")]
    usage: Option<RawUsage>,
}

#[derive(Debug, Deserialize, Default)]
struct RawVolumeList {
    #[serde(rename = "Volumes")]
    volumes: Option<Vec<RawVolume>>,
}

fn map_volume(v: RawVolume, in_use: &BTreeSet<String>) -> Volume {
    Volume {
        in_use: in_use.contains(&v.name),
        name: v.name,
        driver: v.driver.unwrap_or_else(|| "local".into()),
        mountpoint: v.mountpoint.unwrap_or_default(),
        created: v.created_at.unwrap_or_default(),
        scope: v.scope.unwrap_or_else(|| "local".into()),
        labels: v.labels.unwrap_or_default(),
        // -1 rather than 0: "unknown" and "empty" are different, and only
        // `system df` reports real sizes.
        size: v.usage.and_then(|u| u.size).unwrap_or(-1),
    }
}

/// List volumes, marking those a container currently mounts.
///
/// Usage is derived from the container list rather than the volume list
/// because `/volumes` does not report it — and the UI must not offer to remove
/// a volume that is in use.
pub async fn list(client: &Client) -> Result<Vec<Volume>> {
    let raw: RawVolumeList = client.json(Req::get("/volumes")).await?;
    let in_use = mounted_volume_names(client).await;
    Ok(raw
        .volumes
        .unwrap_or_default()
        .into_iter()
        .map(|v| map_volume(v, &in_use))
        .collect())
}

async fn mounted_volume_names(client: &Client) -> BTreeSet<String> {
    let Ok(containers) = crate::containers::list(client, true).await else {
        return BTreeSet::new();
    };
    containers
        .iter()
        .flat_map(|c| c.mounts.iter())
        .filter(|m| m.kind == "volume")
        .filter_map(|m| m.name.clone())
        .collect()
}

pub async fn inspect(client: &Client, name: &str) -> Result<InspectResult> {
    client.json(Req::get(format!("/volumes/{name}"))).await
}

pub async fn create(
    client: &Client,
    name: &str,
    driver: Option<&str>,
    labels: &BTreeMap<String, String>,
) -> Result<Volume> {
    let body = json!({
        "Name": name,
        "Driver": driver.unwrap_or("local"),
        "Labels": labels,
    });
    let raw: RawVolume = client.json(Req::post("/volumes/create").json_body(body)).await?;
    Ok(map_volume(raw, &BTreeSet::new()))
}

pub async fn remove(client: &Client, name: &str, force: bool) -> Result<()> {
    client
        .action(Req::delete(format!("/volumes/{name}")).flag("force", force))
        .await
}

#[derive(Debug, Deserialize, Default)]
struct RawPrune {
    #[serde(rename = "VolumesDeleted")]
    deleted: Option<Vec<String>>,
    #[serde(rename = "SpaceReclaimed")]
    reclaimed: Option<i64>,
}

pub async fn prune(client: &Client) -> Result<PruneReport> {
    let raw: RawPrune = client.json(Req::post("/volumes/prune")).await?;
    Ok(PruneReport {
        kind: "volumes".into(),
        removed: raw.deleted.unwrap_or_default().len() as i64,
        reclaimed: raw.reclaimed.unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_a_volume_and_marks_it_in_use() {
        let raw: RawVolume = serde_json::from_value(json!({
            "Name": "pgdata",
            "Driver": "local",
            "Mountpoint": "/var/lib/docker/volumes/pgdata/_data",
            "CreatedAt": "2026-01-01T00:00:00Z",
            "Scope": "local",
            "UsageData": {"Size": 4096}
        }))
        .unwrap();
        let in_use: BTreeSet<String> = ["pgdata".to_string()].into_iter().collect();
        let v = map_volume(raw, &in_use);
        assert!(v.in_use);
        assert_eq!(v.size, 4096);
    }

    #[test]
    fn an_unmounted_volume_is_not_in_use() {
        let raw: RawVolume = serde_json::from_value(json!({"Name": "orphan"})).unwrap();
        let v = map_volume(raw, &BTreeSet::new());
        assert!(!v.in_use);
        // Unknown, not empty.
        assert_eq!(v.size, -1);
        assert_eq!(v.driver, "local");
    }
}
