//! Discovering what a source engine holds.
//!
//! Docker Desktop → Hopper migration starts here: find the other engine, list
//! what it has, and hand the user a selection. The source endpoint is pinned
//! into the plan so a daemon coming up or down between scan and run cannot
//! silently redirect the migration somewhere else.

use docker::client::Client;
use docker::Endpoint;
use model::{MigrationItem, MigrationKind, MigrationScan};
use std::time::Duration;

/// Where other engines commonly listen, most likely first.
pub fn candidate_endpoints(home: &str) -> Vec<Endpoint> {
    if cfg!(windows) {
        return vec![
            Endpoint::Npipe {
                path: r"\\.\pipe\docker_engine".into(),
            },
            Endpoint::Npipe {
                path: r"\\.\pipe\podman-machine-default".into(),
            },
        ];
    }
    let mut paths = Vec::new();

    // Docker Desktop's per-user socket.
    paths.push(format!("{home}/.docker/run/docker.sock"));
    // Colima.
    paths.push(format!("{home}/.colima/default/docker.sock"));
    // Rancher Desktop.
    paths.push(format!("{home}/.rd/docker.sock"));

    if cfg!(target_os = "macos") {
        // Current Podman publishes its machine API through a temporary socket;
        // keep the persistent path for machines created by older releases.
        if let Some(temp) = non_empty_env("TMPDIR")
            .or_else(|| Some(std::env::temp_dir().to_string_lossy().into_owned()))
        {
            paths.push(format!("{temp}/podman/podman-machine-default-api.sock"));
        }
        paths.push(format!(
            "{home}/.local/share/containers/podman/machine/podman.sock"
        ));
    }

    if cfg!(target_os = "linux") {
        // Rootless sockets are the normal desktop installation and must be
        // considered before system-wide fallbacks.
        if let Some(runtime) = non_empty_env("XDG_RUNTIME_DIR") {
            paths.push(format!("{runtime}/podman/podman.sock"));
            paths.push(format!("{runtime}/docker.sock"));
        }
        paths.push("/run/podman/podman.sock".into());
        // A Podman machine created by an older desktop install can keep its
        // API socket here even when XDG_RUNTIME_DIR is not exported.
        paths.push(format!(
            "{home}/.local/share/containers/podman/machine/podman.sock"
        ));
    }

    // The classic Docker socket remains the final Unix fallback.
    paths.push("/var/run/docker.sock".into());

    paths
        .into_iter()
        .map(|path| Endpoint::Unix { path })
        .collect()
}

fn non_empty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

/// Whether this endpoint is the engine we are migrating *into*.
///
/// Migrating an engine onto itself would copy every image over the top of
/// itself and waste a lot of disk proving nothing.
pub fn is_same_engine(source: &Endpoint, destination: &Endpoint) -> bool {
    match (source, destination) {
        (Endpoint::Unix { path: a }, Endpoint::Unix { path: b }) => a == b,
        (Endpoint::Npipe { path: a }, Endpoint::Npipe { path: b }) => a == b,
        (
            Endpoint::Tcp {
                host: ha, port: pa, ..
            },
            Endpoint::Tcp {
                host: hb, port: pb, ..
            },
        ) => ha == hb && pa == pb,
        _ => false,
    }
}

/// Find the first reachable engine that is not the destination.
pub async fn find_source(destination: &Endpoint, home: &str) -> Option<Endpoint> {
    for candidate in candidate_endpoints(home) {
        if is_same_engine(&candidate, destination) {
            continue;
        }
        if let Endpoint::Unix { path } = &candidate {
            if !std::path::Path::new(path).exists() {
                continue;
            }
        }
        let client = Client::new(candidate.clone());
        // Discovery runs across several possible engines. A stale socket must
        // not make the Import screen wait through the normal operation
        // deadline for every candidate.
        client.set_timeout(Duration::from_secs(3));
        if client.ping().await.is_ok() {
            return Some(candidate);
        }
    }
    None
}

fn human_size(bytes: i64) -> String {
    if bytes <= 0 {
        return String::new();
    }
    let mb = bytes as f64 / (1024.0 * 1024.0);
    if mb >= 1024.0 {
        format!("{:.1} GB", mb / 1024.0)
    } else {
        format!("{mb:.0} MB")
    }
}

/// List everything migratable on the source.
pub async fn scan(destination: &Endpoint, home: &str) -> MigrationScan {
    let Some(source) = find_source(destination, home).await else {
        return MigrationScan {
            available: false,
            message: Some(
                "No other Docker engine was found to migrate from. Start Docker Desktop \
                 (or Colima, or Rancher Desktop) and scan again."
                    .into(),
            ),
            ..Default::default()
        };
    };

    let client = Client::new(source.clone());
    client.set_timeout(Duration::from_secs(10));
    let mut scan = MigrationScan {
        available: true,
        source: Some(source.describe()),
        source_endpoint: Some(source.clone().into()),
        ..Default::default()
    };

    match docker::images::list(&client, false).await {
        Ok(list) => {
            scan.images = list
                .iter()
                .filter(|i| !i.dangling)
                .map(|i| MigrationItem {
                    kind: MigrationKind::Image,
                    id: i.id.clone(),
                    name: i.display_name(),
                    detail: Some(human_size(i.size)).filter(|s| !s.is_empty()),
                })
                .collect();
        }
        Err(e) => record_error(&mut scan, "images", e.message),
    }
    match docker::volumes::list(&client).await {
        Ok(list) => {
            scan.volumes = list
                .iter()
                .map(|v| MigrationItem {
                    kind: MigrationKind::Volume,
                    id: v.name.clone(),
                    name: v.name.clone(),
                    detail: Some(human_size(v.size)).filter(|s| !s.is_empty()),
                })
                .collect();
        }
        Err(e) => record_error(&mut scan, "volumes", e.message),
    }
    match docker::networks::list(&client).await {
        Ok(list) => {
            scan.networks = list
                .iter()
                // Docker's own networks exist on every engine already.
                .filter(|n| !n.is_builtin())
                .map(|n| MigrationItem {
                    kind: MigrationKind::Network,
                    id: n.id.clone(),
                    name: n.name.clone(),
                    detail: Some(n.driver.clone()),
                })
                .collect();
        }
        Err(e) => record_error(&mut scan, "networks", e.message),
    }
    match docker::containers::list(&client, true).await {
        Ok(list) => {
            scan.containers = list
                .iter()
                .map(|c| MigrationItem {
                    kind: MigrationKind::Container,
                    id: c.id.clone(),
                    name: c.name.clone(),
                    detail: Some(c.image.clone()),
                })
                .collect();
        }
        Err(e) => record_error(&mut scan, "containers", e.message),
    }

    if scan.message.is_none()
        && scan.images.is_empty()
        && scan.volumes.is_empty()
        && scan.networks.is_empty()
        && scan.containers.is_empty()
    {
        scan.message = Some("That engine has nothing to migrate.".into());
    }
    scan
}

fn record_error(scan: &mut MigrationScan, kind: &str, error: String) {
    let detail = format!("Could not read {kind} from the source engine: {error}");
    scan.message = Some(match scan.message.take() {
        Some(previous) => format!("{previous} {detail}"),
        None => detail,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(not(windows))]
    fn candidates_cover_the_engines_people_actually_run() {
        let paths: Vec<String> = candidate_endpoints("/Users/x")
            .iter()
            .filter_map(|e| e.path().map(str::to_string))
            .collect();
        assert!(
            paths.iter().any(|p| p.contains(".docker/run")),
            "Docker Desktop"
        );
        assert!(paths.iter().any(|p| p.contains(".colima")), "Colima");
        assert!(paths.iter().any(|p| p.contains(".rd/")), "Rancher Desktop");
        assert!(paths.iter().any(|p| p == "/var/run/docker.sock"));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn macos_candidates_include_podman_machine_sockets() {
        let paths: Vec<String> = candidate_endpoints("/Users/x")
            .iter()
            .filter_map(|e| e.path().map(str::to_string))
            .collect();
        assert!(paths
            .iter()
            .any(|p| p.ends_with("/podman/podman-machine-default-api.sock")));
        assert!(paths
            .iter()
            .any(|p| p.ends_with("/podman/machine/podman.sock")));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn linux_candidates_include_system_podman_socket() {
        let paths: Vec<String> = candidate_endpoints("/home/x")
            .iter()
            .filter_map(|e| e.path().map(str::to_string))
            .collect();
        assert!(paths.iter().any(|p| p == "/run/podman/podman.sock"));
        assert!(paths
            .iter()
            .any(|p| { p == "/home/x/.local/share/containers/podman/machine/podman.sock" }));
    }

    #[test]
    fn an_engine_is_recognized_as_itself() {
        let a = Endpoint::Unix {
            path: "/x.sock".into(),
        };
        let b = Endpoint::Unix {
            path: "/x.sock".into(),
        };
        assert!(is_same_engine(&a, &b));
    }

    #[test]
    fn different_engines_are_distinguished() {
        let a = Endpoint::Unix {
            path: "/a.sock".into(),
        };
        let b = Endpoint::Unix {
            path: "/b.sock".into(),
        };
        assert!(!is_same_engine(&a, &b));
    }

    #[test]
    fn tcp_engines_compare_on_host_and_port_ignoring_tls() {
        let a = Endpoint::Tcp {
            host: "h".into(),
            port: 2375,
            tls: false,
        };
        let b = Endpoint::Tcp {
            host: "h".into(),
            port: 2375,
            tls: true,
        };
        // Same daemon reached with and without TLS is still one daemon.
        assert!(is_same_engine(&a, &b));
        let c = Endpoint::Tcp {
            host: "h".into(),
            port: 2376,
            tls: true,
        };
        assert!(!is_same_engine(&a, &c));
    }

    #[test]
    fn transports_of_different_kinds_are_never_the_same_engine() {
        let unix = Endpoint::Unix { path: "/x".into() };
        let tcp = Endpoint::Tcp {
            host: "h".into(),
            port: 1,
            tls: false,
        };
        assert!(!is_same_engine(&unix, &tcp));
    }

    #[test]
    fn sizes_render_in_the_unit_that_reads_best() {
        assert_eq!(human_size(0), "");
        assert_eq!(human_size(-1), "");
        assert_eq!(human_size(150 * 1024 * 1024), "150 MB");
        assert_eq!(human_size(2 * 1024 * 1024 * 1024), "2.0 GB");
    }

    #[test]
    fn partial_scan_failures_are_kept_visible_to_the_user() {
        let mut scan = MigrationScan::default();
        record_error(&mut scan, "images", "permission denied".into());
        record_error(&mut scan, "volumes", "timed out".into());
        let message = scan.message.unwrap();
        assert!(message.contains("images"));
        assert!(message.contains("volumes"));
        assert!(!message.contains("nothing to migrate"));
    }

    // Unix only: Windows candidates are named pipes a Unix destination cannot
    // filter out, and the Windows CI image runs Docker on `docker_engine`.
    #[tokio::test]
    #[cfg(not(windows))]
    async fn a_scan_finds_no_source_when_every_candidate_is_the_destination_or_absent() {
        // Pin the destination to the one socket that might really exist on the
        // test machine, so it is filtered as the same engine; the home-based
        // candidates live under a directory that does not exist. This keeps the
        // test hermetic whether or not a daemon is running here.
        let destination = Endpoint::Unix {
            path: "/var/run/docker.sock".into(),
        };
        let scan = scan(&destination, "/nonexistent-hopper-home").await;
        assert!(!scan.available);
        assert!(scan.message.unwrap().contains("No other Docker engine"));
    }

    #[test]
    fn the_destination_is_never_offered_as_its_own_migration_source() {
        // Whatever engine Hopper is migrating *into* must be excluded, or the
        // scan would offer to copy it onto itself.
        let dest = Endpoint::Unix {
            path: "/var/run/docker.sock".into(),
        };
        let filtered: Vec<Endpoint> = candidate_endpoints("/Users/x")
            .into_iter()
            .filter(|c| !is_same_engine(c, &dest))
            .collect();
        assert!(!filtered
            .iter()
            .any(|c| c.path() == Some("/var/run/docker.sock")));
    }

    #[test]
    #[cfg(windows)]
    fn windows_migration_sources_use_named_pipes() {
        assert!(candidate_endpoints("")
            .iter()
            .all(|endpoint| matches!(endpoint, Endpoint::Npipe { .. })));
    }
}
