//! Copying a selection from one engine into another.
//!
//! Images move as tar streams through `/images/get` and `/images/load`;
//! volumes through a helper container and `/archive`. Every step reports
//! progress, and a per-item failure is recorded rather than aborting the run —
//! one unreadable volume must not cost the user the other nineteen.

use docker::client::Client;
use model::{MigrationPhase, MigrationPlan, MigrationProgress};

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
    let binds: Vec<&String> = sources
        .iter()
        .filter(|s| s.starts_with('/'))
        .collect();
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
    for (i, reference) in plan.images.iter().enumerate() {
        report(step(
            MigrationPhase::Images,
            reference,
            i,
            total,
            format!("Copying {reference}"),
        ));
        match docker::images::save(source, std::slice::from_ref(reference)).await {
            Ok(tar) => match docker::images::load(destination, tar).await {
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
    let existing = docker::networks::list(source).await.unwrap_or_default();

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
