//! Bringing a Docker install across into Hopper.
//!
//! Two destinations, because Hopper has two backends. Against an Engine API
//! engine the copy is socket to socket. Against Apple's runtime there is no
//! socket to copy into, so images land on disk and are loaded back through
//! `container image load`.
//!
//! The source is always read-only. Nothing here removes anything from Docker.

use docker::Endpoint;
use model::{MigrationPhase, MigrationPlan, MigrationProgress, MigrationScan, RuntimeKind};

use crate::facade::Host;
use crate::runtime::Backend;

/// A progress sink, matching the one `migrate` reports through.
pub type Report<'a> = &'a mut (dyn FnMut(MigrationProgress) + Send);

/// The endpoint used to exclude "migrating an engine onto itself".
///
/// Apple's runtime owns no socket, so nothing on disk can be it — a path that
/// cannot exist keeps every real Docker socket eligible as a source.
fn destination_endpoint(host: &Host) -> Endpoint {
    match host.runtime_kind() {
        RuntimeKind::Apple => Endpoint::Unix {
            path: "\u{0}apple-containers".into(),
        },
        RuntimeKind::EngineApi => host.client().endpoint(),
    }
}

impl Host {
    /// What the other engine holds, ready for the user to choose from.
    pub async fn import_scan(&self) -> MigrationScan {
        let home = std::env::var("HOME").unwrap_or_default();
        migrate::scan(&destination_endpoint(self), &home).await
    }

    /// Copy the selection across, reporting as it goes.
    ///
    /// Ordering matters: images before containers, because a container cannot
    /// start without its image; networks before containers for the same
    /// reason.
    pub async fn import_run(&self, plan: &MigrationPlan, report: Report<'_>) -> String {
        let Some(source_endpoint) = plan.source.clone() else {
            return "No source engine was pinned for this import.".into();
        };
        let source = docker::client::Client::new(source_endpoint.into());

        // (images, networks, containers)
        let (images, networks, containers) = match self.backend() {
            #[cfg(target_os = "macos")]
            Backend::Apple(cli) => {
                let images = migrate::apple::import_images(&source, &cli, plan, report).await;
                let containers =
                    migrate::apple::import_containers(&source, &cli, plan, report).await;
                // Apple attaches containers to networks at creation, so there
                // is nothing to recreate ahead of them.
                skipped(report, MigrationPhase::Networks, &plan.networks,
                    "Apple Containers attaches networks when a container is created, so this one was not recreated separately.");
                (images, 0, containers)
            }
            Backend::EngineApi => {
                let destination = self.client();
                let networks =
                    migrate::run::migrate_networks(&source, &destination, plan, report).await;
                let images = migrate::run::migrate_images(&source, &destination, plan, report).await;
                skipped(report, MigrationPhase::Containers, &plan.containers,
                    "Recreating containers on an Engine API engine is not implemented yet — the image came across, so `docker run` it.");
                (images, networks, 0)
            }
        };

        // Volume *contents* need a helper container on both sides and are not
        // copied yet. Saying so is the difference between a missing feature
        // and a silent data loss the user only notices later.
        skipped(report, MigrationPhase::Volumes, &plan.volumes,
            "Volume contents are not copied yet. Create the volume on this engine and move the data yourself before starting the container that needs it.");

        let summary = summarize(images, networks, containers);
        report(migrate::run::finished(summary.clone()));
        summary
    }
}

/// Report every item of a kind as skipped, with the reason.
///
/// A selection that quietly does nothing is worse than one that refuses: the
/// user ticked these, and the summary would otherwise imply they arrived.
fn skipped(report: Report<'_>, phase: MigrationPhase, items: &[String], why: &str) {
    let total = items.len();
    for (i, item) in items.iter().enumerate() {
        report(MigrationProgress {
            phase,
            item: item.clone(),
            done: i,
            total,
            message: "Skipped".into(),
            error: None,
            warning: Some(why.to_string()),
            finished: false,
        });
    }
}

/// One sentence describing what came across.
///
/// Counts only, and only the non-zero ones — "0 networks" in a summary is
/// noise when the user never selected any.
pub fn summarize(images: usize, networks: usize, containers: usize) -> String {
    let mut parts = Vec::new();
    let plural = |n: usize, one: &str, many: &str| {
        if n == 1 { format!("{n} {one}") } else { format!("{n} {many}") }
    };
    if images > 0 {
        parts.push(plural(images, "image", "images"));
    }
    if networks > 0 {
        parts.push(plural(networks, "network", "networks"));
    }
    if containers > 0 {
        parts.push(plural(containers, "container", "containers"));
    }
    if parts.is_empty() {
        return "Nothing was imported.".into();
    }
    format!("Imported {}.", join(&parts))
}

fn join(parts: &[String]) -> String {
    match parts {
        [] => String::new(),
        [a] => a.clone(),
        [a, b] => format!("{a} and {b}"),
        [rest @ .., last] => format!("{}, and {last}", rest.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frames(items: &[&str], phase: MigrationPhase) -> Vec<MigrationProgress> {
        let owned: Vec<String> = items.iter().map(|s| s.to_string()).collect();
        let mut out = Vec::new();
        {
            let mut sink = |p: MigrationProgress| out.push(p);
            skipped(&mut sink, phase, &owned, "because reasons");
        }
        out
    }

    #[test]
    fn every_skipped_item_is_reported_with_a_reason() {
        // Silence here would read as success for something that never moved.
        let f = frames(&["pgdata", "cache"], MigrationPhase::Volumes);
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].item, "pgdata");
        assert_eq!(f[0].message, "Skipped");
        assert_eq!(f[0].warning.as_deref(), Some("because reasons"));
        assert!(f[0].error.is_none(), "skipping is not a failure");
        assert!(!f[1].finished);
    }

    #[test]
    fn nothing_selected_reports_nothing() {
        assert!(frames(&[], MigrationPhase::Volumes).is_empty());
    }

    #[test]
    fn a_summary_names_only_what_actually_moved() {
        assert_eq!(summarize(3, 0, 0), "Imported 3 images.");
        assert_eq!(summarize(0, 0, 0), "Nothing was imported.");
    }

    #[test]
    fn singulars_read_correctly() {
        assert_eq!(summarize(1, 0, 0), "Imported 1 image.");
        assert_eq!(summarize(1, 1, 1), "Imported 1 image, 1 network, and 1 container.");
    }

    #[test]
    fn two_kinds_join_with_and_rather_than_a_comma() {
        assert_eq!(summarize(2, 1, 0), "Imported 2 images and 1 network.");
    }

    #[test]
    fn three_kinds_use_a_serial_comma() {
        assert_eq!(summarize(2, 3, 4), "Imported 2 images, 3 networks, and 4 containers.");
    }
}
