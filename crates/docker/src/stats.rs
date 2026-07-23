//! Live container stats, reduced to the display values the meters show.
//!
//! The daemon streams cumulative counters; turning those into a CPU
//! percentage is a delta against the previous sample, and every edge case
//! (first sample, a restarted container resetting counters, a missing
//! `system_cpu_usage` on some platforms) has to produce a sane number rather
//! than a NaN or a spike. That arithmetic is pure, so it tests directly.

use crate::client::{Client, Req};
use crate::error::Result;
use model::ContainerStats;
use serde::Deserialize;

#[derive(Debug, Deserialize, Default)]
struct CpuUsage {
    total_usage: Option<u64>,
    percpu_usage: Option<Vec<u64>>,
}

#[derive(Debug, Deserialize, Default)]
struct CpuStats {
    cpu_usage: Option<CpuUsage>,
    system_cpu_usage: Option<u64>,
    online_cpus: Option<u64>,
}

#[derive(Debug, Deserialize, Default)]
struct MemoryStats {
    usage: Option<u64>,
    limit: Option<u64>,
    stats: Option<std::collections::BTreeMap<String, u64>>,
}

#[derive(Debug, Deserialize, Default)]
struct NetworkStat {
    rx_bytes: Option<u64>,
    tx_bytes: Option<u64>,
}

#[derive(Debug, Deserialize, Default)]
struct BlkioEntry {
    op: Option<String>,
    value: Option<u64>,
}

#[derive(Debug, Deserialize, Default)]
struct BlkioStats {
    io_service_bytes_recursive: Option<Vec<BlkioEntry>>,
}

#[derive(Debug, Deserialize, Default)]
struct PidsStats {
    current: Option<u64>,
}

#[derive(Debug, Deserialize, Default)]
pub struct RawStats {
    id: Option<String>,
    cpu_stats: Option<CpuStats>,
    precpu_stats: Option<CpuStats>,
    memory_stats: Option<MemoryStats>,
    networks: Option<std::collections::BTreeMap<String, NetworkStat>>,
    blkio_stats: Option<BlkioStats>,
    pids_stats: Option<PidsStats>,
}

/// CPU percent from the cumulative counters, the way `docker stats` computes
/// it: the container's delta over the system's delta, scaled by core count.
fn cpu_percent(cur: &CpuStats, pre: &CpuStats) -> f64 {
    let total = cur.cpu_usage.as_ref().and_then(|u| u.total_usage).unwrap_or(0);
    let pre_total = pre.cpu_usage.as_ref().and_then(|u| u.total_usage).unwrap_or(0);
    let system = cur.system_cpu_usage.unwrap_or(0);
    let pre_system = pre.system_cpu_usage.unwrap_or(0);

    // No baseline yet. The formula would still produce a number, but it would
    // be the container's *lifetime average* share rather than its current
    // rate — which reads as a spike on the first frame of every stream. A
    // running system always has a non-zero system counter, so zero here
    // reliably means "no previous sample".
    if pre_system == 0 {
        return 0.0;
    }

    // A restarted container resets its counters; a negative delta means the
    // previous sample is meaningless, so report 0 rather than a wild spike.
    let cpu_delta = total.saturating_sub(pre_total) as f64;
    let system_delta = system.saturating_sub(pre_system) as f64;
    if cpu_delta <= 0.0 || system_delta <= 0.0 {
        return 0.0;
    }

    let cores = cur
        .online_cpus
        .or_else(|| {
            cur.cpu_usage
                .as_ref()
                .and_then(|u| u.percpu_usage.as_ref())
                .map(|v| v.len() as u64)
        })
        .filter(|c| *c > 0)
        .unwrap_or(1) as f64;

    (cpu_delta / system_delta) * cores * 100.0
}

/// Memory usage minus the page cache, which is what Docker Desktop shows.
/// Counting cache makes an idle container look like it is using its whole
/// limit.
fn memory_usage(mem: &MemoryStats) -> u64 {
    let usage = mem.usage.unwrap_or(0);
    let cache = mem
        .stats
        .as_ref()
        .and_then(|s| {
            // cgroup v2 calls it `inactive_file`; v1 calls it `cache`.
            s.get("inactive_file").or_else(|| s.get("cache")).copied()
        })
        .unwrap_or(0);
    usage.saturating_sub(cache)
}

pub fn reduce(raw: &RawStats) -> ContainerStats {
    let empty = CpuStats::default();
    let cur = raw.cpu_stats.as_ref().unwrap_or(&empty);
    let pre = raw.precpu_stats.as_ref().unwrap_or(&empty);

    let mem = raw.memory_stats.as_ref();
    let mem_usage = mem.map(memory_usage).unwrap_or(0);
    let mem_limit = mem.and_then(|m| m.limit).unwrap_or(0);

    let (net_rx, net_tx) = raw
        .networks
        .as_ref()
        .map(|nets| {
            nets.values().fold((0u64, 0u64), |(rx, tx), n| {
                (
                    rx + n.rx_bytes.unwrap_or(0),
                    tx + n.tx_bytes.unwrap_or(0),
                )
            })
        })
        .unwrap_or((0, 0));

    let (block_read, block_write) = raw
        .blkio_stats
        .as_ref()
        .and_then(|b| b.io_service_bytes_recursive.as_ref())
        .map(|entries| {
            entries.iter().fold((0u64, 0u64), |(r, w), e| {
                match e.op.as_deref().map(str::to_ascii_lowercase).as_deref() {
                    Some("read") => (r + e.value.unwrap_or(0), w),
                    Some("write") => (r, w + e.value.unwrap_or(0)),
                    _ => (r, w),
                }
            })
        })
        .unwrap_or((0, 0));

    ContainerStats {
        id: raw.id.clone().unwrap_or_default(),
        cpu_percent: cpu_percent(cur, pre),
        mem_usage,
        mem_limit,
        mem_percent: if mem_limit > 0 {
            (mem_usage as f64 / mem_limit as f64) * 100.0
        } else {
            0.0
        },
        net_rx,
        net_tx,
        block_read,
        block_write,
        pids: raw
            .pids_stats
            .as_ref()
            .and_then(|p| p.current)
            .unwrap_or(0),
    }
}

/// Stream stats for one container until the callback returns `false` or the
/// future is dropped.
pub async fn stream<F>(client: &Client, id: &str, mut on_sample: F) -> Result<()>
where
    F: FnMut(ContainerStats) -> bool,
{
    client
        .ndjson::<RawStats, _>(
            Req::get(format!("/containers/{id}/stats"))
                .flag("stream", true)
                .no_timeout(),
            move |raw| on_sample(reduce(&raw)),
        )
        .await
}

/// A single stats sample. Docker still needs two reads to compute CPU, so this
/// asks for `stream=false`, which makes the daemon do that itself.
pub async fn once(client: &Client, id: &str) -> Result<ContainerStats> {
    let raw: RawStats = client
        .json(Req::get(format!("/containers/{id}/stats")).query("stream", "false"))
        .await?;
    Ok(reduce(&raw))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(v: serde_json::Value) -> RawStats {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn cpu_percent_scales_by_core_count() {
        let raw = parse(json!({
            "cpu_stats": {
                "cpu_usage": {"total_usage": 200},
                "system_cpu_usage": 2000,
                "online_cpus": 4
            },
            "precpu_stats": {
                "cpu_usage": {"total_usage": 100},
                "system_cpu_usage": 1000
            }
        }));
        // 100/1000 * 4 * 100 = 40%
        assert!((reduce(&raw).cpu_percent - 40.0).abs() < 0.001);
    }

    #[test]
    fn cpu_percent_falls_back_to_percpu_length_when_online_cpus_is_absent() {
        let raw = parse(json!({
            "cpu_stats": {
                "cpu_usage": {"total_usage": 200, "percpu_usage": [1, 2]},
                "system_cpu_usage": 2000
            },
            "precpu_stats": {
                "cpu_usage": {"total_usage": 100},
                "system_cpu_usage": 1000
            }
        }));
        // 100/1000 * 2 * 100 = 20%
        assert!((reduce(&raw).cpu_percent - 20.0).abs() < 0.001);
    }

    #[test]
    fn the_first_sample_reports_zero_rather_than_a_spike() {
        let raw = parse(json!({
            "cpu_stats": {"cpu_usage": {"total_usage": 500}, "system_cpu_usage": 5000},
            "precpu_stats": {}
        }));
        // With no previous system value the delta is meaningless.
        assert_eq!(reduce(&raw).cpu_percent, 0.0);
    }

    #[test]
    fn counters_going_backwards_report_zero_not_a_huge_number() {
        // A restarted container resets its cumulative counters.
        let raw = parse(json!({
            "cpu_stats": {"cpu_usage": {"total_usage": 10}, "system_cpu_usage": 100},
            "precpu_stats": {"cpu_usage": {"total_usage": 9999}, "system_cpu_usage": 99999}
        }));
        assert_eq!(reduce(&raw).cpu_percent, 0.0);
    }

    #[test]
    fn a_completely_empty_sample_produces_zeroes_not_nan() {
        let s = reduce(&parse(json!({})));
        assert_eq!(s.cpu_percent, 0.0);
        assert_eq!(s.mem_percent, 0.0);
        assert!(!s.cpu_percent.is_nan());
        assert!(!s.mem_percent.is_nan());
    }

    #[test]
    fn memory_excludes_the_page_cache() {
        let raw = parse(json!({
            "memory_stats": {
                "usage": 1000,
                "limit": 2000,
                "stats": {"inactive_file": 400}
            }
        }));
        let s = reduce(&raw);
        assert_eq!(s.mem_usage, 600);
        assert!((s.mem_percent - 30.0).abs() < 0.001);
    }

    #[test]
    fn memory_falls_back_to_the_cgroup_v1_cache_key() {
        let raw = parse(json!({
            "memory_stats": {"usage": 1000, "limit": 2000, "stats": {"cache": 250}}
        }));
        assert_eq!(reduce(&raw).mem_usage, 750);
    }

    #[test]
    fn network_counters_sum_across_interfaces() {
        let raw = parse(json!({
            "networks": {
                "eth0": {"rx_bytes": 100, "tx_bytes": 200},
                "eth1": {"rx_bytes": 10, "tx_bytes": 20}
            }
        }));
        let s = reduce(&raw);
        assert_eq!(s.net_rx, 110);
        assert_eq!(s.net_tx, 220);
    }

    #[test]
    fn block_io_splits_reads_from_writes_case_insensitively() {
        let raw = parse(json!({
            "blkio_stats": {
                "io_service_bytes_recursive": [
                    {"op": "Read", "value": 500},
                    {"op": "write", "value": 300},
                    {"op": "Sync", "value": 999}
                ]
            }
        }));
        let s = reduce(&raw);
        assert_eq!(s.block_read, 500);
        assert_eq!(s.block_write, 300);
    }

    #[test]
    fn a_zero_memory_limit_does_not_divide_by_zero() {
        let raw = parse(json!({"memory_stats": {"usage": 100, "limit": 0}}));
        assert_eq!(reduce(&raw).mem_percent, 0.0);
    }
}
