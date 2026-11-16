use model::EngineStats;

pub const SCRIPT: &str = r#"set -eu
awk '/MemTotal:/ {t=$2} /MemAvailable:/ {a=$2} END {print "memory", t, a}' /proc/meminfo
df -Pk / | awk 'NR==2 {print "disk", $2, $3}'
awk '{print "load", $1}' /proc/loadavg
"#;

pub fn parse(output: &str) -> Option<EngineStats> {
    let mut memory = None;
    let mut disk = None;
    let mut load = None;
    for line in output.lines() {
        let columns: Vec<_> = line.split_whitespace().collect();
        match columns.as_slice() {
            ["memory", total, available] => {
                memory = Some((total.parse::<u64>().ok()?, available.parse::<u64>().ok()?))
            }
            ["disk", total, used] => {
                disk = Some((total.parse::<u64>().ok()?, used.parse::<u64>().ok()?))
            }
            ["load", value] => load = Some(value.parse::<f64>().ok()?),
            _ => {}
        }
    }
    let (mem_total_kb, mem_avail_kb) = memory?;
    let (disk_total_kb, disk_used_kb) = disk?;
    let load1 = load?;
    if mem_total_kb == 0
        || mem_avail_kb > mem_total_kb
        || disk_total_kb == 0
        || disk_used_kb > disk_total_kb
        || !load1.is_finite()
        || load1 < 0.0
    {
        return None;
    }
    Some(EngineStats {
        mem_total_kb,
        mem_avail_kb,
        disk_total_kb,
        disk_used_kb,
        load1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_or_invalid_samples_are_never_reported_as_a_healthy_empty_vm() {
        assert!(parse("memory 2048 1024\ndisk 10000 2000\nload 0.10").is_some());
        for input in [
            "",
            "memory 2048 1024",
            "memory 1 2\ndisk 3 1\nload 0",
            "memory 2 1\ndisk 3 1\nload NaN",
        ] {
            assert!(parse(input).is_none());
        }
    }
}
