//! Importing from Docker into Apple Containers.
//!
//! The Docker→Docker path streams tars between two Engine APIs. Apple has no
//! Engine API, so everything lands on disk first: `/images/get` produces a tar,
//! the tar goes to a temporary file, and `container image load` reads it back.
//! Slower than a socket-to-socket copy, and the only way in.
//!
//! Nothing here deletes from the source. An import that goes wrong should cost
//! time, not the user's containers.

use std::path::{Path, PathBuf};

use apple::Cli;
use docker::client::Client;
use model::{MigrationPhase, MigrationPlan, MigrationProgress};

use crate::run::Report;

fn step(
    phase: MigrationPhase,
    item: &str,
    done: usize,
    total: usize,
    message: impl Into<String>,
) -> MigrationProgress {
    MigrationProgress {
        phase,
        item: item.to_string(),
        done,
        total,
        message: message.into(),
        error: None,
        warning: None,
        finished: false,
    }
}

fn failed(phase: MigrationPhase, item: &str, done: usize, total: usize, error: String) -> MigrationProgress {
    let mut p = step(phase, item, done, total, "Failed");
    p.error = Some(error);
    p
}

/// A scratch directory for the tars, cleaned up when the import ends.
///
/// Under the Hopper directory rather than `/tmp`: an image tar can be several
/// gigabytes, and `/tmp` on macOS is not always where the free space is.
pub fn scratch_dir() -> PathBuf {
    let root = std::env::var("HOPPER_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs_home()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".hopper")
        });
    root.join("import")
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// A filename that cannot collide or escape the scratch directory.
///
/// Image references carry `/` and `:`, both of which would otherwise write
/// outside the directory or produce an unopenable name.
pub fn tar_name(reference: &str) -> String {
    let safe: String = reference
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' })
        .collect();
    format!("{safe}.tar")
}

/// Copy the selected images from Docker into Apple's runtime.
pub async fn import_images(
    source: &Client,
    cli: &Cli,
    plan: &MigrationPlan,
    report: Report<'_>,
) -> usize {
    let total = plan.images.len();
    if total == 0 {
        return 0;
    }
    let dir = scratch_dir();
    if let Err(e) = tokio::fs::create_dir_all(&dir).await {
        report(failed(MigrationPhase::Images, "", 0, total, format!("could not make a scratch directory: {e}")));
        return 0;
    }

    let mut copied = 0;
    for (i, reference) in plan.images.iter().enumerate() {
        report(step(MigrationPhase::Images, reference, i, total, format!("Copying {reference}")));
        let path = dir.join(tar_name(reference));

        match copy_one_image(source, cli, reference, &path).await {
            Ok(()) => copied += 1,
            Err(e) => report(failed(MigrationPhase::Images, reference, i, total, e)),
        }
        // The tar is large and already consumed either way.
        let _ = tokio::fs::remove_file(&path).await;
    }
    let _ = tokio::fs::remove_dir(&dir).await;
    copied
}

async fn copy_one_image(
    source: &Client,
    cli: &Cli,
    reference: &str,
    path: &Path,
) -> Result<(), String> {
    let tar = docker::images::save(source, std::slice::from_ref(&reference.to_string()))
        .await
        .map_err(|e| format!("could not export {reference} from Docker: {}", e.message))?;
    tokio::fs::write(path, &tar)
        .await
        .map_err(|e| format!("could not stage {reference}: {e}"))?;
    apple::images::load(cli, path)
        .await
        .map_err(|e| format!("Apple Containers refused {reference}: {}", e.message))
}

/// Recreate the selected containers on Apple's runtime.
///
/// Recreated, not moved: a container's writable layer does not travel, so what
/// comes across is the image, the ports, the mounts and the environment. That
/// is what makes a stack come back up; the rest is by definition scratch.
pub async fn import_containers(
    source: &Client,
    cli: &Cli,
    plan: &MigrationPlan,
    report: Report<'_>,
) -> usize {
    let total = plan.containers.len();
    if total == 0 {
        return 0;
    }
    let existing = docker::containers::list(source, true).await.unwrap_or_default();
    let mut created = 0;

    for (i, id) in plan.containers.iter().enumerate() {
        let Some(c) = existing.iter().find(|c| &c.id == id || &c.name == id) else {
            continue;
        };
        report(step(MigrationPhase::Containers, &c.name, i, total, format!("Recreating {}", c.name)));

        let input = to_run_input(c);
        // Anything Apple cannot honour is a warning on the item, not a failure.
        for note in apple::containers::unsupported(&input) {
            let mut p = step(MigrationPhase::Containers, &c.name, i, total, "Recreated with changes");
            p.warning = Some(note);
            report(p);
        }
        if let Some(warning) = crate::run::bind_warning(&c.name, &bind_sources(c)) {
            let mut p = step(MigrationPhase::Containers, &c.name, i, total, "Recreated with changes");
            p.warning = Some(warning);
            report(p);
        }

        match apple::containers::run(cli, &input).await {
            Ok(_) => created += 1,
            Err(e) if e.is_conflict() => created += 1,
            Err(e) => report(failed(MigrationPhase::Containers, &c.name, i, total, e.message)),
        }
    }
    created
}

/// The host paths a container bind-mounts, which will not exist in the guest.
pub fn bind_sources(c: &model::Container) -> Vec<String> {
    c.mounts
        .iter()
        .filter(|m| m.kind == "bind")
        .map(|m| m.source.clone())
        .collect()
}

/// A running container, described as the request that would recreate it.
pub fn to_run_input(c: &model::Container) -> model::RunInput {
    let ports = c
        .ports
        .iter()
        .filter_map(|p| {
            // Only published ports can be recreated; an unpublished one has no
            // host side to ask for.
            p.public_port.map(|host| model::PortMapping {
                host: host.to_string(),
                container: p.private_port.to_string(),
                proto: Some(p.proto.clone()),
            })
        })
        .collect();

    let volumes = c
        .mounts
        .iter()
        .map(|m| model::VolumeMapping {
            host: m.name.clone().unwrap_or_else(|| m.source.clone()),
            container: m.destination.clone(),
            ro: !m.rw,
        })
        .collect();

    model::RunInput {
        image: c.image.clone(),
        name: Some(c.name.clone()),
        env: Vec::new(),
        ports,
        volumes,
        // The image's own entrypoint is the right default; a command copied
        // from a running container often names a path only that image has.
        command: None,
        restart: None,
        auto_remove: false,
        network: c.networks.first().cloned().filter(|n| n != "bridge" && n != "default"),
        workdir: None,
        user: None,
        hostname: None,
        limits: Default::default(),
        labels: c.labels.clone(),
        tty: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::{Container, ContainerState, Health, Mount, Port};

    fn container() -> Container {
        Container {
            id: "abc".into(),
            name: "web".into(),
            image: "nginx:latest".into(),
            image_id: "sha256:1".into(),
            command: "nginx -g daemon off;".into(),
            created: 0,
            state: ContainerState::Running,
            status: "Up".into(),
            health: Health::None,
            ports: vec![],
            labels: Default::default(),
            mounts: vec![],
            networks: vec![],
            compose_project: None,
            compose_service: None,
        }
    }

    #[test]
    fn an_image_reference_becomes_a_safe_filename() {
        // `/` would write outside the scratch directory; `:` is not openable
        // in a way we want to rely on.
        let name = tar_name("ghcr.io/wess/hopper:1.2.3");
        assert!(!name.contains('/'));
        assert!(!name.contains(':'));
        assert!(name.ends_with(".tar"));
        assert_eq!(name, "ghcr.io_wess_hopper_1.2.3.tar");
    }

    #[test]
    fn different_references_do_not_collide() {
        assert_ne!(tar_name("a/b"), tar_name("a/c"));
    }

    #[test]
    fn only_published_ports_are_recreated() {
        let mut c = container();
        c.ports = vec![
            Port { ip: None, private_port: 80, public_port: Some(8080), proto: "tcp".into() },
            // Exposed but not published: there is no host port to ask for.
            Port { ip: None, private_port: 443, public_port: None, proto: "tcp".into() },
        ];
        let input = to_run_input(&c);
        assert_eq!(input.ports.len(), 1);
        assert_eq!(input.ports[0].host, "8080");
        assert_eq!(input.ports[0].container, "80");
    }

    #[test]
    fn a_named_volume_travels_by_name_and_a_bind_by_path() {
        let mut c = container();
        c.mounts = vec![
            Mount { kind: "volume".into(), source: "/var/lib/docker/volumes/data/_data".into(), destination: "/data".into(), mode: "rw".into(), rw: true, name: Some("data".into()) },
            Mount { kind: "bind".into(), source: "/Users/wess/code".into(), destination: "/src".into(), mode: "ro".into(), rw: false, name: None },
        ];
        let input = to_run_input(&c);
        assert_eq!(input.volumes[0].host, "data", "a named volume must not travel as its host path");
        assert!(!input.volumes[0].ro);
        assert_eq!(input.volumes[1].host, "/Users/wess/code");
        assert!(input.volumes[1].ro);
    }

    #[test]
    fn bind_mounts_are_the_only_ones_warned_about() {
        let mut c = container();
        c.mounts = vec![
            Mount { kind: "bind".into(), source: "/Users/wess/code".into(), destination: "/src".into(), mode: "rw".into(), rw: true, name: None },
            Mount { kind: "volume".into(), source: "x".into(), destination: "/d".into(), mode: "rw".into(), rw: true, name: Some("x".into()) },
        ];
        assert_eq!(bind_sources(&c), vec!["/Users/wess/code".to_string()]);
    }

    #[test]
    fn the_default_bridge_is_not_carried_across() {
        // Apple has its own default network; asking for Docker's `bridge`
        // would fail rather than do the obvious thing.
        let mut c = container();
        c.networks = vec!["bridge".into()];
        assert!(to_run_input(&c).network.is_none());

        c.networks = vec!["my-stack".into()];
        assert_eq!(to_run_input(&c).network.as_deref(), Some("my-stack"));
    }

    #[test]
    fn labels_survive_so_compose_stacks_regroup() {
        let mut c = container();
        c.labels.insert("com.docker.compose.project".into(), "shop".into());
        let input = to_run_input(&c);
        assert_eq!(input.labels.get("com.docker.compose.project").map(String::as_str), Some("shop"));
    }
}
