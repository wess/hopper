//! Discovering what a source engine holds.
//!
//! Docker Desktop → Hopper migration starts here: find the other engine, list
//! what it has, and hand the user a selection. The source endpoint is pinned
//! into the plan so a daemon coming up or down between scan and run cannot
//! silently redirect the migration somewhere else.

use docker::client::Client;
use docker::Endpoint;
use model::{MigrationItem, MigrationKind, MigrationScan};

/// Where other engines commonly listen, most likely first.
pub fn candidate_endpoints(home: &str) -> Vec<Endpoint> {
    [
        // Docker Desktop's per-user socket.
        format!("{home}/.docker/run/docker.sock"),
        // Colima.
        format!("{home}/.colima/default/docker.sock"),
        // Rancher Desktop.
        format!("{home}/.rd/docker.sock"),
        // The classic system socket.
        "/var/run/docker.sock".to_string(),
    ]
    .into_iter()
    .map(|path| Endpoint::Unix { path })
    .collect()
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
    let mut scan = MigrationScan {
        available: true,
        source: Some(source.describe()),
        source_endpoint: Some(source.clone().into()),
        ..Default::default()
    };

    if let Ok(list) = docker::images::list(&client, false).await {
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
    if let Ok(list) = docker::volumes::list(&client).await {
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
    if let Ok(list) = docker::networks::list(&client).await {
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
    if let Ok(list) = docker::containers::list(&client, true).await {
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

    if scan.images.is_empty()
        && scan.volumes.is_empty()
        && scan.networks.is_empty()
        && scan.containers.is_empty()
    {
        scan.message = Some("That engine has nothing to migrate.".into());
    }
    scan
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidates_cover_the_engines_people_actually_run() {
        let paths: Vec<String> = candidate_endpoints("/Users/x")
            .iter()
            .filter_map(|e| e.path().map(str::to_string))
            .collect();
        assert!(paths.iter().any(|p| p.contains(".docker/run")), "Docker Desktop");
        assert!(paths.iter().any(|p| p.contains(".colima")), "Colima");
        assert!(paths.iter().any(|p| p.contains(".rd/")), "Rancher Desktop");
        assert!(paths.iter().any(|p| p == "/var/run/docker.sock"));
    }

    #[test]
    fn an_engine_is_recognized_as_itself() {
        let a = Endpoint::Unix { path: "/x.sock".into() };
        let b = Endpoint::Unix { path: "/x.sock".into() };
        assert!(is_same_engine(&a, &b));
    }

    #[test]
    fn different_engines_are_distinguished() {
        let a = Endpoint::Unix { path: "/a.sock".into() };
        let b = Endpoint::Unix { path: "/b.sock".into() };
        assert!(!is_same_engine(&a, &b));
    }

    #[test]
    fn tcp_engines_compare_on_host_and_port_ignoring_tls() {
        let a = Endpoint::Tcp { host: "h".into(), port: 2375, tls: false };
        let b = Endpoint::Tcp { host: "h".into(), port: 2375, tls: true };
        // Same daemon reached with and without TLS is still one daemon.
        assert!(is_same_engine(&a, &b));
        let c = Endpoint::Tcp { host: "h".into(), port: 2376, tls: true };
        assert!(!is_same_engine(&a, &c));
    }

    #[test]
    fn transports_of_different_kinds_are_never_the_same_engine() {
        let unix = Endpoint::Unix { path: "/x".into() };
        let tcp = Endpoint::Tcp { host: "h".into(), port: 1, tls: false };
        assert!(!is_same_engine(&unix, &tcp));
    }

    #[test]
    fn sizes_render_in_the_unit_that_reads_best() {
        assert_eq!(human_size(0), "");
        assert_eq!(human_size(-1), "");
        assert_eq!(human_size(150 * 1024 * 1024), "150 MB");
        assert_eq!(human_size(2 * 1024 * 1024 * 1024), "2.0 GB");
    }

    #[tokio::test]
    async fn a_scan_finds_no_source_when_every_candidate_is_the_destination_or_absent() {
        // Pin the destination to the one socket that might really exist on the
        // test machine, so it is filtered as the same engine; the home-based
        // candidates live under a directory that does not exist. This keeps the
        // test hermetic whether or not a daemon is running here.
        let destination = Endpoint::Unix { path: "/var/run/docker.sock".into() };
        let scan = scan(&destination, "/nonexistent-hopper-home").await;
        assert!(!scan.available);
        assert!(scan.message.unwrap().contains("No other Docker engine"));
    }

    #[test]
    fn the_destination_is_never_offered_as_its_own_migration_source() {
        // Whatever engine Hopper is migrating *into* must be excluded, or the
        // scan would offer to copy it onto itself.
        let dest = Endpoint::Unix { path: "/var/run/docker.sock".into() };
        let filtered: Vec<Endpoint> = candidate_endpoints("/Users/x")
            .into_iter()
            .filter(|c| !is_same_engine(c, &dest))
            .collect();
        assert!(!filtered.iter().any(|c| c.path() == Some("/var/run/docker.sock")));
    }
}
