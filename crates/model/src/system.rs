//! System-level shapes: version, info, disk usage, events, prune reports.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemVersion {
    pub version: String,
    pub api_version: String,
    pub os: String,
    pub arch: String,
    pub kernel_version: String,
    pub go_version: String,
    pub build_time: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    pub name: String,
    pub containers: i64,
    pub containers_running: i64,
    pub containers_paused: i64,
    pub containers_stopped: i64,
    pub images: i64,
    pub server_version: String,
    pub operating_system: String,
    pub architecture: String,
    pub ncpu: i64,
    pub mem_total: i64,
    pub docker_root_dir: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageBucket {
    pub count: i64,
    pub size: i64,
    pub reclaimable: i64,
}

/// `docker system df` — totals plus what a prune would reclaim.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskUsage {
    pub images: UsageBucket,
    pub containers: UsageBucket,
    pub volumes: UsageBucket,
    pub build_cache: UsageBucket,
}

impl DiskUsage {
    pub fn total_size(&self) -> i64 {
        self.images.size + self.containers.size + self.volumes.size + self.build_cache.size
    }

    pub fn total_reclaimable(&self) -> i64 {
        self.images.reclaimable
            + self.containers.reclaimable
            + self.volumes.reclaimable
            + self.build_cache.reclaimable
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DockerEvent {
    /// container | image | volume | network | daemon | …
    #[serde(rename = "type")]
    pub kind: String,
    pub action: String,
    /// Name or id of the subject.
    pub actor: String,
    /// Unix seconds.
    pub time: i64,
    /// Pre-formatted one-liner for the activity feed.
    pub message: String,
}

/// A pruned-resource report (containers/images/volumes/networks/build cache).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PruneReport {
    pub kind: String,
    pub removed: i64,
    /// Bytes.
    pub reclaimed: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disk_usage_totals_every_bucket() {
        let bucket = |size, reclaimable| UsageBucket {
            count: 1,
            size,
            reclaimable,
        };
        let df = DiskUsage {
            images: bucket(100, 40),
            containers: bucket(10, 5),
            volumes: bucket(1000, 0),
            build_cache: bucket(7, 7),
        };
        assert_eq!(df.total_size(), 1117);
        assert_eq!(df.total_reclaimable(), 52);
    }
}
