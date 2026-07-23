//! Display formatting.
//!
//! Pure, so the rounding and pluralization rules that are easy to get subtly
//! wrong are pinned by tests rather than eyeballed in the UI.

/// A compact popularity count: 968 → "968", 21340 → "21.3k", 2_100_000 → "2.1M".
pub fn count(n: i64) -> String {
    match n {
        n if n < 0 => "—".into(),
        n if n < 1_000 => n.to_string(),
        n if n < 1_000_000 => format!("{:.1}k", n as f64 / 1_000.0),
        n => format!("{:.1}M", n as f64 / 1_000_000.0),
    }
}

/// Human byte sizes, in the binary units Docker reports.
pub fn bytes(n: i64) -> String {
    if n < 0 {
        // -1 means "unknown", which is not the same as empty.
        return "—".into();
    }
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{n} B")
    } else if value >= 10.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// A coarse relative time from a unix-seconds timestamp.
pub fn ago(unix_seconds: i64) -> String {
    if unix_seconds <= 0 {
        return "unknown".into();
    }
    let now = chrono::Utc::now().timestamp();
    let secs = (now - unix_seconds).max(0);
    let plural = |n: i64, unit: &str| {
        if n == 1 {
            format!("1 {unit} ago")
        } else {
            format!("{n} {unit}s ago")
        }
    };
    match secs {
        s if s < 60 => "just now".into(),
        s if s < 3_600 => plural(s / 60, "minute"),
        s if s < 86_400 => plural(s / 3_600, "hour"),
        s if s < 2_592_000 => plural(s / 86_400, "day"),
        s if s < 31_536_000 => plural(s / 2_592_000, "month"),
        s => plural(s / 31_536_000, "year"),
    }
}

/// Published ports, condensed for a list row.
#[allow(dead_code)]
pub fn ports(ports: &[model::Port]) -> String {
    let mut shown: Vec<String> = ports
        .iter()
        .filter_map(|p| {
            p.public_port
                .map(|public| format!("{public}→{}/{}", p.private_port, p.proto))
        })
        .collect();
    shown.sort();
    shown.dedup();
    shown.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_are_compact() {
        assert_eq!(count(0), "0");
        assert_eq!(count(968), "968");
        assert_eq!(count(21_340), "21.3k");
        assert_eq!(count(2_100_000), "2.1M");
        assert_eq!(count(-1), "—");
    }

    #[test]
    fn byte_sizes_use_binary_units() {
        assert_eq!(bytes(0), "0 B");
        assert_eq!(bytes(512), "512 B");
        assert_eq!(bytes(1024), "1.0 KB");
        assert_eq!(bytes(1536), "1.5 KB");
        assert_eq!(bytes(1024 * 1024), "1.0 MB");
        assert_eq!(bytes(142 * 1024 * 1024), "142 MB");
    }

    #[test]
    fn large_values_drop_the_decimal_place() {
        // Ten or more of a unit does not need a tenth to be useful.
        assert_eq!(bytes(15 * 1024 * 1024), "15 MB");
        assert_eq!(bytes(2 * 1024 * 1024 * 1024), "2.0 GB");
    }

    #[test]
    fn an_unknown_size_is_not_rendered_as_zero() {
        assert_eq!(bytes(-1), "—");
    }

    #[test]
    fn relative_times_are_singular_at_one() {
        let now = chrono::Utc::now().timestamp();
        assert_eq!(ago(now - 30), "just now");
        assert_eq!(ago(now - 60), "1 minute ago");
        assert_eq!(ago(now - 120), "2 minutes ago");
        assert_eq!(ago(now - 3_600), "1 hour ago");
        assert_eq!(ago(now - 86_400), "1 day ago");
    }

    #[test]
    fn a_missing_timestamp_says_so_rather_than_claiming_1970() {
        assert_eq!(ago(0), "unknown");
        assert_eq!(ago(-5), "unknown");
    }

    #[test]
    fn a_future_timestamp_does_not_render_negatively() {
        let future = chrono::Utc::now().timestamp() + 10_000;
        assert_eq!(ago(future), "just now");
    }

    #[test]
    fn ports_show_only_published_mappings() {
        let list = vec![
            model::Port {
                ip: None,
                private_port: 80,
                public_port: Some(8080),
                proto: "tcp".into(),
            },
            // Unpublished: nothing to show the user.
            model::Port {
                ip: None,
                private_port: 9000,
                public_port: None,
                proto: "tcp".into(),
            },
        ];
        assert_eq!(ports(&list), "8080→80/tcp");
    }

    #[test]
    fn duplicate_mappings_across_interfaces_are_collapsed() {
        // Docker reports one entry per bound address; the user needs one line.
        let list = vec![
            model::Port {
                ip: Some("0.0.0.0".into()),
                private_port: 80,
                public_port: Some(8080),
                proto: "tcp".into(),
            },
            model::Port {
                ip: Some("::".into()),
                private_port: 80,
                public_port: Some(8080),
                proto: "tcp".into(),
            },
        ];
        assert_eq!(ports(&list), "8080→80/tcp");
    }

    #[test]
    fn no_published_ports_renders_empty_rather_than_a_stray_separator() {
        assert_eq!(ports(&[]), "");
    }
}
