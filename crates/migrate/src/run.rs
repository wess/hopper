//! Copying a selection from one engine into another.
//!
//! Images move as tar streams through `/images/get` and `/images/load`;
//! volumes through a helper container and `/archive`. Every step reports
//! progress, and a per-item failure is recorded rather than aborting the run —
//! one unreadable volume must not cost the user the other nineteen.

use docker::client::Client;
use model::{ContainerState, MigrationPhase, MigrationPlan, MigrationProgress};
use serde_json::Value;
use uuid::Uuid;

/// A progress sink.
///
/// `Send` because the import runs on the tokio runtime while the view that
/// draws the progress lives on the gpui thread.
pub type Report<'a> = &'a mut (dyn FnMut(MigrationProgress) + Send);

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

/// Bind mounts name host paths that will not exist on the destination, so the
/// container is copied but the mount is called out.
pub fn bind_warning(name: &str, sources: &[String]) -> Option<String> {
    let binds: Vec<&String> = sources.iter().filter(|s| s.starts_with('/')).collect();
    if binds.is_empty() {
        return None;
    }
    Some(format!(
        "{name} bind-mounts {} from your Mac. Make sure the path is shared with \
         Hopper's engine, or the container will see an empty directory.",
        binds
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

/// Copy the selected images across.
pub async fn migrate_images(
    source: &Client,
    destination: &Client,
    plan: &MigrationPlan,
    report: Report<'_>,
) -> usize {
    let total = plan.images.len();
    let mut copied = 0;
    let scratch = match tempfile::Builder::new()
        .prefix(&format!("hopper-migrate-{}-", Uuid::new_v4()))
        .tempdir_in(std::env::temp_dir())
    {
        Ok(dir) => dir,
        Err(e) => {
            for reference in &plan.images {
                let mut p = step(MigrationPhase::Images, reference, 0, total, "Failed");
                p.error = Some(format!("Could not create migration scratch directory: {e}"));
                report(p);
            }
            return 0;
        }
    };
    for (i, reference) in plan.images.iter().enumerate() {
        report(step(
            MigrationPhase::Images,
            reference,
            i,
            total,
            format!("Copying {reference}"),
        ));
        let archive = scratch.path().join(format!("image-{i}.tar"));
        match docker::images::save_to(source, std::slice::from_ref(reference), &archive).await {
            Ok(()) => match docker::images::load_file(destination, &archive).await {
                Ok(()) => copied += 1,
                Err(e) => {
                    let mut p = step(MigrationPhase::Images, reference, i, total, "Failed");
                    p.error = Some(e.message);
                    report(p);
                }
            },
            Err(e) => {
                let mut p = step(MigrationPhase::Images, reference, i, total, "Failed");
                p.error = Some(e.message);
                report(p);
            }
        }
    }
    copied
}

/// Recreate the selected networks.
pub async fn migrate_networks(
    source: &Client,
    destination: &Client,
    plan: &MigrationPlan,
    report: Report<'_>,
) -> usize {
    let total = plan.networks.len();
    let mut created = 0;
    let existing = match docker::networks::list(source).await {
        Ok(existing) => existing,
        Err(e) => {
            let mut p = step(MigrationPhase::Networks, "networks", 0, total, "Failed");
            p.error = Some(format!(
                "Could not read networks from the source: {}",
                e.message
            ));
            report(p);
            return 0;
        }
    };

    for (i, id) in plan.networks.iter().enumerate() {
        let Some(net) = existing.iter().find(|n| &n.id == id || &n.name == id) else {
            continue;
        };
        report(step(
            MigrationPhase::Networks,
            &net.name,
            i,
            total,
            format!("Creating {}", net.name),
        ));
        let input = model::NetworkCreateInput {
            name: net.name.clone(),
            driver: Some(net.driver.clone()),
            internal: net.internal,
            attachable: net.attachable,
            subnet: net.ipam.first().and_then(|c| c.subnet.clone()),
            gateway: net.ipam.first().and_then(|c| c.gateway.clone()),
        };
        match docker::networks::create(destination, &input).await {
            Ok(_) => created += 1,
            Err(e) if e.is_conflict() => {
                // Already there: that is a success for a migration.
                created += 1;
            }
            Err(e) => {
                let mut p = step(MigrationPhase::Networks, &net.name, i, total, "Failed");
                p.error = Some(e.message);
                report(p);
            }
        }
    }
    created
}

/// Recreate selected containers from their portable list representation.
///
/// A container's writable layer is intentionally not copied. Images, names,
/// ports, mounts, networks, and labels are recreated from the source so the
/// destination can start the same workload; bind mounts are called out because
/// their host paths may not exist on the destination machine.
pub async fn migrate_containers(
    source: &Client,
    destination: &Client,
    plan: &MigrationPlan,
    report: Report<'_>,
) -> usize {
    let total = plan.containers.len();
    if total == 0 {
        return 0;
    }
    let existing = match docker::containers::list(source, true).await {
        Ok(existing) => existing,
        Err(e) => {
            let mut p = step(MigrationPhase::Containers, "containers", 0, total, "Failed");
            p.error = Some(format!(
                "Could not read containers from the source: {}",
                e.message
            ));
            report(p);
            return 0;
        }
    };
    let mut created = 0;

    for (i, id) in plan.containers.iter().enumerate() {
        let Some(container) = existing.iter().find(|c| &c.id == id || &c.name == id) else {
            let mut p = step(MigrationPhase::Containers, id, i, total, "Failed");
            p.error = Some("The source container no longer exists.".into());
            report(p);
            continue;
        };
        report(step(
            MigrationPhase::Containers,
            &container.name,
            i,
            total,
            format!("Recreating {}", container.name),
        ));

        if let Some(warning) = bind_warning(&container.name, &bind_sources(container)) {
            let mut p = step(
                MigrationPhase::Containers,
                &container.name,
                i,
                total,
                "Recreated with changes",
            );
            p.warning = Some(warning);
            report(p);
        }

        let input = match inspect_run_input(source, container).await {
            Ok(input) => input,
            Err(e) => {
                let mut p = step(
                    MigrationPhase::Containers,
                    &container.name,
                    i,
                    total,
                    "Failed",
                );
                p.error = Some(format!(
                    "Could not inspect the source container: {}",
                    e.message
                ));
                report(p);
                continue;
            }
        };
        match docker::containers::create(destination, &input).await {
            Ok(id) => {
                if container.state.is_up() && container.state != ContainerState::Paused {
                    if let Err(e) = docker::containers::start(destination, &id).await {
                        let mut p = step(
                            MigrationPhase::Containers,
                            &container.name,
                            i,
                            total,
                            "Failed",
                        );
                        p.error = Some(e.message);
                        report(p);
                        continue;
                    }
                }
                created += 1;
            }
            Err(e) if e.is_conflict() => created += 1,
            Err(e) => {
                let mut p = step(
                    MigrationPhase::Containers,
                    &container.name,
                    i,
                    total,
                    "Failed",
                );
                p.error = Some(e.message);
                report(p);
            }
        }
    }
    created
}

/// The host paths a container bind-mounts, which may not exist on the
/// destination host.
pub fn bind_sources(c: &model::Container) -> Vec<String> {
    c.mounts
        .iter()
        .filter(|m| m.kind == "bind")
        .map(|m| m.source.clone())
        .collect()
}

/// A container, described as the portable request that recreates it.
pub fn to_run_input(c: &model::Container) -> model::RunInput {
    let ports = c
        .ports
        .iter()
        .filter_map(|p| {
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
        // The list endpoint's command is the best fallback when inspect is
        // unavailable. The richer inspect path below replaces it with the
        // daemon's exact Config.Cmd and Config.Entrypoint where possible.
        command: (!c.command.trim().is_empty()).then(|| c.command.clone()),
        entrypoint: None,
        restart: None,
        auto_remove: false,
        network: c
            .networks
            .first()
            .cloned()
            .filter(|n| n != "bridge" && n != "default"),
        workdir: None,
        user: None,
        hostname: None,
        limits: Default::default(),
        labels: c.labels.clone(),
        tty: false,
    }
}

/// Fill the fields that `/containers/json` intentionally omits. A migration
/// that only copies the list projection can create a container that looks
/// right but starts a different process with an empty environment.
pub(crate) async fn inspect_run_input(
    source: &Client,
    c: &model::Container,
) -> docker::error::Result<model::RunInput> {
    let mut input = to_run_input(c);
    let raw = docker::containers::inspect(source, &c.id).await?;
    let config = raw.get("Config").ok_or_else(|| {
        docker::error::DockerError::decode("Source container has no Config section.")
    })?;
    input.env = strings(config.get("Env"));
    input.command = shell_words(config.get("Cmd"));
    input.entrypoint = shell_words(config.get("Entrypoint"));
    input.workdir = nonempty_string(config.get("WorkingDir"));
    input.user = nonempty_string(config.get("User"));
    input.hostname = nonempty_string(config.get("Hostname"));
    input.tty = config.get("Tty").and_then(Value::as_bool).unwrap_or(false);
    if let Some(restart) = raw
        .get("HostConfig")
        .and_then(|h| h.get("RestartPolicy"))
        .and_then(|p| p.get("Name"))
        .and_then(Value::as_str)
        .filter(|r| !r.is_empty() && *r != "no")
    {
        input.restart = Some(restart.to_string());
    }
    Ok(input)
}

fn strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

fn nonempty_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

/// Encode an inspected argv as the shell-like command format used by
/// `RunInput`. Quoting every argument keeps spaces and shell punctuation from
/// changing meaning when the destination builder tokenizes it.
fn shell_words(value: Option<&Value>) -> Option<String> {
    let args = strings(value);
    (!args.is_empty()).then(|| {
        args.iter()
            .map(|arg| format!("'{}'", arg.replace('\'', "'\\''")))
            .collect::<Vec<_>>()
            .join(" ")
    })
}

/// The final frame.
pub fn finished(message: impl Into<String>) -> MigrationProgress {
    MigrationProgress {
        phase: MigrationPhase::Done,
        item: String::new(),
        done: 0,
        total: 0,
        message: message.into(),
        error: None,
        warning: None,
        finished: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bind_mount_is_called_out_so_it_can_be_shared() {
        let warning = bind_warning("web", &["/Users/x/code".into()]).unwrap();
        assert!(warning.contains("/Users/x/code"));
        assert!(warning.contains("empty directory"));
    }

    #[test]
    fn named_volumes_produce_no_warning() {
        // A named volume travels with the migration; nothing to warn about.
        assert!(bind_warning("db", &["pgdata".into()]).is_none());
        assert!(bind_warning("db", &[]).is_none());
    }

    #[test]
    fn several_binds_are_listed_together() {
        let warning = bind_warning("app", &["/a".into(), "/b".into(), "named".into()]).unwrap();
        assert!(warning.contains("/a, /b"));
        assert!(!warning.contains("named"));
    }

    #[test]
    fn the_final_frame_is_marked_finished() {
        let f = finished("Migrated 3 images.");
        assert!(f.finished);
        assert_eq!(f.phase, MigrationPhase::Done);
    }

    #[test]
    fn progress_steps_carry_their_position() {
        let p = step(MigrationPhase::Images, "nginx", 2, 5, "Copying");
        assert_eq!(p.done, 2);
        assert_eq!(p.total, 5);
        assert!(!p.finished);
        assert!(p.error.is_none());
    }
}
