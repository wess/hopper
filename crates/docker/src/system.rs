//! System-level operations: version, info, disk usage, prune-all, and the live
//! event stream that drives the activity feed and auto-refresh.

use crate::client::{Client, Req};
use crate::error::Result;
use model::{DiskUsage, DockerEvent, PruneReport, SystemInfo, SystemVersion, UsageBucket};
use serde::Deserialize;
use serde_json::Value;

pub async fn ping(client: &Client) -> bool {
    client.ping().await.is_ok()
}

#[derive(Debug, Deserialize, Default)]
struct RawVersion {
    #[serde(rename = "Version")]
    version: Option<String>,
    #[serde(rename = "ApiVersion")]
    api_version: Option<String>,
    #[serde(rename = "Os")]
    os: Option<String>,
    #[serde(rename = "Arch")]
    arch: Option<String>,
    #[serde(rename = "KernelVersion")]
    kernel_version: Option<String>,
    #[serde(rename = "GoVersion")]
    go_version: Option<String>,
    #[serde(rename = "BuildTime")]
    build_time: Option<String>,
}

pub async fn version(client: &Client) -> Result<SystemVersion> {
    let v: RawVersion = client.json(Req::get("/version")).await?;
    Ok(SystemVersion {
        version: v.version.unwrap_or_default(),
        api_version: v.api_version.unwrap_or_default(),
        os: v.os.unwrap_or_default(),
        arch: v.arch.unwrap_or_default(),
        kernel_version: v.kernel_version.unwrap_or_default(),
        go_version: v.go_version.unwrap_or_default(),
        build_time: v.build_time.unwrap_or_default(),
    })
}

#[derive(Debug, Deserialize, Default)]
struct RawInfo {
    #[serde(rename = "Name")]
    name: Option<String>,
    #[serde(rename = "Containers")]
    containers: Option<i64>,
    #[serde(rename = "ContainersRunning")]
    containers_running: Option<i64>,
    #[serde(rename = "ContainersPaused")]
    containers_paused: Option<i64>,
    #[serde(rename = "ContainersStopped")]
    containers_stopped: Option<i64>,
    #[serde(rename = "Images")]
    images: Option<i64>,
    #[serde(rename = "ServerVersion")]
    server_version: Option<String>,
    #[serde(rename = "OperatingSystem")]
    operating_system: Option<String>,
    #[serde(rename = "Architecture")]
    architecture: Option<String>,
    #[serde(rename = "NCPU")]
    ncpu: Option<i64>,
    #[serde(rename = "MemTotal")]
    mem_total: Option<i64>,
    #[serde(rename = "DockerRootDir")]
    docker_root_dir: Option<String>,
}

pub async fn info(client: &Client) -> Result<SystemInfo> {
    let i: RawInfo = client.json(Req::get("/info")).await?;
    Ok(SystemInfo {
        name: i.name.unwrap_or_default(),
        containers: i.containers.unwrap_or_default(),
        containers_running: i.containers_running.unwrap_or_default(),
        containers_paused: i.containers_paused.unwrap_or_default(),
        containers_stopped: i.containers_stopped.unwrap_or_default(),
        images: i.images.unwrap_or_default(),
        server_version: i.server_version.unwrap_or_default(),
        operating_system: i.operating_system.unwrap_or_default(),
        architecture: i.architecture.unwrap_or_default(),
        ncpu: i.ncpu.unwrap_or_default(),
        mem_total: i.mem_total.unwrap_or_default(),
        docker_root_dir: i.docker_root_dir.unwrap_or_default(),
    })
}

#[derive(Debug, Deserialize, Default)]
struct RawDfImage {
    #[serde(rename = "Size")]
    size: Option<i64>,
    #[serde(rename = "Containers")]
    containers: Option<i64>,
}

#[derive(Debug, Deserialize, Default)]
struct RawDfContainer {
    #[serde(rename = "SizeRw")]
    size_rw: Option<i64>,
}

#[derive(Debug, Deserialize, Default)]
struct RawUsageData {
    #[serde(rename = "Size")]
    size: Option<i64>,
}

#[derive(Debug, Deserialize, Default)]
struct RawDfVolume {
    #[serde(rename = "UsageData")]
    usage_data: Option<RawUsageData>,
}

#[derive(Debug, Deserialize, Default)]
struct RawDfCache {
    #[serde(rename = "Size")]
    size: Option<i64>,
    #[serde(rename = "InUse")]
    in_use: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
struct RawDf {
    #[serde(rename = "Images")]
    images: Option<Vec<RawDfImage>>,
    #[serde(rename = "Containers")]
    containers: Option<Vec<RawDfContainer>>,
    #[serde(rename = "Volumes")]
    volumes: Option<Vec<RawDfVolume>>,
    #[serde(rename = "BuildCache")]
    build_cache: Option<Vec<RawDfCache>>,
    #[serde(rename = "LayersSize")]
    layers_size: Option<i64>,
}

fn map_df(d: RawDf) -> DiskUsage {
    let images = d.images.unwrap_or_default();
    let containers = d.containers.unwrap_or_default();
    let volumes = d.volumes.unwrap_or_default();
    let cache = d.build_cache.unwrap_or_default();

    // `LayersSize` already accounts for shared layers; summing per-image sizes
    // double-counts them, so prefer it when the daemon sends it.
    let image_size = d
        .layers_size
        .unwrap_or_else(|| images.iter().filter_map(|i| i.size).sum());
    let image_active: i64 = images
        .iter()
        .filter(|i| i.containers.unwrap_or(0) > 0)
        .filter_map(|i| i.size)
        .sum();

    DiskUsage {
        images: UsageBucket {
            count: images.len() as i64,
            size: image_size,
            reclaimable: (image_size - image_active).max(0),
        },
        containers: UsageBucket {
            count: containers.len() as i64,
            size: containers.iter().filter_map(|c| c.size_rw).sum(),
            reclaimable: 0,
        },
        volumes: UsageBucket {
            count: volumes.len() as i64,
            size: volumes
                .iter()
                .filter_map(|v| v.usage_data.as_ref().and_then(|u| u.size))
                .sum(),
            reclaimable: 0,
        },
        build_cache: UsageBucket {
            count: cache.len() as i64,
            size: cache.iter().filter_map(|c| c.size).sum(),
            reclaimable: cache
                .iter()
                .filter(|c| !c.in_use.unwrap_or(false))
                .filter_map(|c| c.size)
                .sum(),
        },
    }
}

pub async fn df(client: &Client) -> Result<DiskUsage> {
    let raw: RawDf = client.json(Req::get("/system/df")).await?;
    Ok(map_df(raw))
}

/// Prune everything reclaimable, returning a per-kind report.
///
/// A failure in one kind must not abort the rest — a network prune that hits a
/// still-attached endpoint should not stop the build cache from being freed.
pub async fn prune_all(client: &Client) -> Vec<PruneReport> {
    let steps = [
        ("containers", "/containers/prune", "ContainersDeleted"),
        ("images", "/images/prune", "ImagesDeleted"),
        ("volumes", "/volumes/prune", "VolumesDeleted"),
        ("networks", "/networks/prune", "NetworksDeleted"),
        ("build cache", "/build/prune", "CachesDeleted"),
    ];
    let mut reports = Vec::with_capacity(steps.len());
    for (kind, path, key) in steps {
        let report = match client.json::<Value>(Req::post(path)).await {
            Ok(raw) => PruneReport {
                kind: kind.into(),
                removed: raw
                    .get(key)
                    .and_then(|v| v.as_array())
                    .map(|a| a.len() as i64)
                    .unwrap_or(0),
                reclaimed: raw
                    .get("SpaceReclaimed")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0),
            },
            Err(_) => PruneReport {
                kind: kind.into(),
                removed: 0,
                reclaimed: 0,
            },
        };
        reports.push(report);
    }
    reports
}

#[derive(Debug, Deserialize, Default)]
struct RawActor {
    #[serde(rename = "ID")]
    id: Option<String>,
    #[serde(rename = "Attributes")]
    attributes: Option<std::collections::BTreeMap<String, String>>,
}

#[derive(Debug, Deserialize, Default)]
struct RawEvent {
    #[serde(rename = "Type")]
    kind: Option<String>,
    #[serde(rename = "Action")]
    action: Option<String>,
    #[serde(rename = "Actor")]
    actor: Option<RawActor>,
    time: Option<i64>,
    status: Option<String>,
    id: Option<String>,
}

fn format_event(e: RawEvent) -> DockerEvent {
    let kind = e.kind.unwrap_or_default();
    let action = e.action.or(e.status).unwrap_or_default();
    let name = e
        .actor
        .as_ref()
        .and_then(|a| a.attributes.as_ref().and_then(|m| m.get("name").cloned()))
        .or_else(|| e.actor.as_ref().and_then(|a| a.id.clone()))
        .or(e.id)
        .unwrap_or_default();
    // Shorten bare hex ids the way Docker does, but leave real names alone.
    let short = if name.len() > 12 && name.chars().all(|c| c.is_ascii_hexdigit()) {
        name[..12].to_string()
    } else {
        name
    };
    DockerEvent {
        message: format!("{kind} {action} {short}").trim().to_string(),
        kind,
        action,
        actor: short,
        time: e.time.unwrap_or_default(),
    }
}

/// Stream daemon events, invoking `on_event` per event. Returning `false` from
/// the callback closes the stream; dropping the future does the same.
pub async fn stream_events<F>(client: &Client, mut on_event: F) -> Result<()>
where
    F: FnMut(DockerEvent) -> bool,
{
    client
        .ndjson::<RawEvent, _>(Req::get("/events").no_timeout(), move |raw| {
            on_event(format_event(raw))
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn df_prefers_layers_size_over_summing_shared_layers() {
        let raw: RawDf = serde_json::from_value(json!({
            "LayersSize": 1000,
            "Images": [
                {"Size": 800, "Containers": 1},
                {"Size": 800, "Containers": 0}
            ],
            "Containers": [{"SizeRw": 50}],
            "Volumes": [{"UsageData": {"Size": 200}}],
            "BuildCache": [{"Size": 30, "InUse": false}, {"Size": 70, "InUse": true}]
        }))
        .unwrap();
        let df = map_df(raw);
        // Not 1600 — the two images share layers.
        assert_eq!(df.images.size, 1000);
        assert_eq!(df.images.reclaimable, 200);
        assert_eq!(df.containers.size, 50);
        assert_eq!(df.volumes.size, 200);
        assert_eq!(df.build_cache.size, 100);
        assert_eq!(df.build_cache.reclaimable, 30);
    }

    #[test]
    fn df_reclaimable_never_goes_negative() {
        let raw: RawDf = serde_json::from_value(json!({
            "LayersSize": 100,
            "Images": [{"Size": 900, "Containers": 2}]
        }))
        .unwrap();
        assert_eq!(map_df(raw).images.reclaimable, 0);
    }

    #[test]
    fn df_handles_a_completely_empty_response() {
        let df = map_df(serde_json::from_value(json!({})).unwrap());
        assert_eq!(df.total_size(), 0);
        assert_eq!(df.total_reclaimable(), 0);
    }

    #[test]
    fn events_prefer_the_actor_name_over_the_id() {
        let raw: RawEvent = serde_json::from_value(json!({
            "Type": "container",
            "Action": "start",
            "Actor": {"ID": "abcdef0123456789", "Attributes": {"name": "web"}},
            "time": 1700000000i64
        }))
        .unwrap();
        let e = format_event(raw);
        assert_eq!(e.actor, "web");
        assert_eq!(e.message, "container start web");
    }

    #[test]
    fn events_shorten_bare_hex_ids_but_not_names() {
        let hex: RawEvent = serde_json::from_value(json!({
            "Type": "image", "Action": "pull",
            "Actor": {"ID": "abcdef0123456789abcdef"}
        }))
        .unwrap();
        assert_eq!(format_event(hex).actor, "abcdef012345");

        // A long name that merely looks hex-ish must survive intact.
        let named: RawEvent = serde_json::from_value(json!({
            "Type": "container", "Action": "die",
            "Actor": {"Attributes": {"name": "deadbeefservice"}}
        }))
        .unwrap();
        assert_eq!(format_event(named).actor, "deadbeefservice");
    }

    #[test]
    fn events_fall_back_to_the_legacy_status_field() {
        let raw: RawEvent = serde_json::from_value(json!({
            "Type": "container", "status": "destroy", "id": "xyz"
        }))
        .unwrap();
        let e = format_event(raw);
        assert_eq!(e.action, "destroy");
        assert_eq!(e.actor, "xyz");
    }
}
