//! Bringing a compose stack up and taking it down.
//!
//! The plan arrives fully resolved, so nothing here parses YAML or decides
//! what a service means — it creates networks, creates volumes, and starts
//! containers in the order it was handed, through the same `Host` methods
//! every other view uses. That is what makes it work identically on Apple's
//! runtime and on an Engine API daemon: neither is named anywhere below.
//!
//! Progress is reported line by line as it happens. A stack of a dozen
//! services takes a while, and a spinner that says nothing for ninety seconds
//! is indistinguishable from one that has hung.

use crate::facade::Host;
use model::{
    ComposePlan, ComposePlanService, ComposeProgress, Container, ContainerState,
    NetworkCreateInput, StreamKind,
};
use std::collections::BTreeSet;

/// Where progress lines go.
pub type Report<'a> = &'a mut (dyn FnMut(ComposeProgress) + Send);

fn line(request_id: &str, text: impl Into<String>) -> ComposeProgress {
    ComposeProgress {
        request_id: request_id.to_string(),
        line: text.into(),
        stream: StreamKind::Stdout,
        done: false,
        error: None,
    }
}

fn problem(request_id: &str, text: impl Into<String>) -> ComposeProgress {
    ComposeProgress {
        request_id: request_id.to_string(),
        line: text.into(),
        stream: StreamKind::Stderr,
        done: false,
        error: None,
    }
}

fn finished(request_id: &str, summary: impl Into<String>, error: Option<String>) -> ComposeProgress {
    ComposeProgress {
        request_id: request_id.to_string(),
        line: summary.into(),
        stream: StreamKind::Stdout,
        done: true,
        error,
    }
}

impl Host {
    /// Create everything the plan needs and start its services in order.
    ///
    /// Returns the one-line summary, which is also the final progress frame.
    pub async fn compose_up(&self, plan: &ComposePlan, report: Report<'_>) -> String {
        let id = plan.project.clone();

        // Everything the file asked for that this engine will not do, said
        // once at the top rather than discovered service by service.
        for warning in plan.all_warnings() {
            report(problem(&id, warning));
        }

        for network in &plan.networks {
            if network.external {
                report(line(&id, format!("Network {} is external, leaving it alone.", network.name)));
                continue;
            }
            match self
                .network_create(&NetworkCreateInput {
                    name: network.name.clone(),
                    internal: network.internal,
                    ..Default::default()
                })
                .await
            {
                Ok(_) => report(line(&id, format!("Network {} created.", network.name))),
                // A network that is already there is the normal case on a
                // second `up`, not a failure. Both backends report it as a
                // 409: the Engine API natively, Apple's CLI via `cli::classify`.
                Err(e) if e.is_conflict() => {
                    report(line(&id, format!("Network {} exists.", network.name)))
                }
                Err(e) => report(problem(
                    &id,
                    format!("Network {} could not be created: {}", network.name, e.message),
                )),
            }
        }

        for volume in &plan.volumes {
            if volume.external {
                report(line(&id, format!("Volume {} is external, leaving it alone.", volume.name)));
                continue;
            }
            match self.volume_create(&volume.name).await {
                Ok(_) => report(line(&id, format!("Volume {} ready.", volume.name))),
                Err(e) if e.is_conflict() => {
                    report(line(&id, format!("Volume {} exists.", volume.name)))
                }
                Err(e) => report(problem(
                    &id,
                    format!("Volume {} could not be created: {}", volume.name, e.message),
                )),
            }
        }

        let existing = self.containers(true).await.unwrap_or_default();
        // Listed once, not once per service: a twelve-service stack would
        // otherwise pull the whole image list twelve times before starting.
        let mut present = self.image_names().await;
        let mut started = 0usize;
        let mut failed = 0usize;

        let runnable = plan.runnable();
        for service in &runnable {
            match self
                .start_service(&id, service, &existing, &mut present, report)
                .await
            {
                Ok(()) => started += 1,
                Err(reason) => {
                    failed += 1;
                    report(problem(&id, reason));
                }
            }
        }

        let skipped = plan.services.len() - runnable.len();
        let summary = summarize(started, failed, skipped);
        // A stack that half came up is a failure worth colouring, even though
        // the services that did start are genuinely running.
        let error = (failed > 0).then(|| summary.clone());
        report(finished(&id, summary.clone(), error));
        summary
    }

    /// One service: recreate its container if it is already there, then run it.
    async fn start_service(
        &self,
        id: &str,
        service: &ComposePlanService,
        existing: &[Container],
        present: &mut BTreeSet<String>,
        report: Report<'_>,
    ) -> Result<(), String> {
        let name = service.run.name.clone().unwrap_or_else(|| service.service.clone());

        // A container that already matches the file is left exactly as it is.
        // Recreating unconditionally would throw away a database on every `up`
        // — and `up` is the command people run to check on a stack.
        if let Some(old) = existing.iter().find(|c| c.name == name) {
            if unchanged(old, service) && old.state == ContainerState::Running {
                report(line(id, format!("{name} is up to date.")));
                return Ok(());
            }
            let _ = self.container_stop(&old.id).await;
            if let Err(e) = self.container_remove(&old.id, true, false).await {
                return Err(format!("{name} could not be replaced: {}", e.message));
            }
            report(line(id, format!("{name} recreated.")));
        }

        // Pull only what is missing. Re-pulling on every `up` would turn a
        // five-second restart into a download.
        if !have_image(present, &service.run.image) {
            report(line(id, format!("Pulling {}…", service.run.image)));
            if let Err(e) = self.pull(id, &service.run.image, |_| {}).await {
                return Err(format!(
                    "{name} was not started: {} could not be pulled ({}).",
                    service.run.image, e.message
                ));
            }
            // Two services sharing an image must not pull it twice, whether or
            // not they spell the tag the same way.
            remember_image(present, &service.run.image);
        }

        match self.container_run(&service.run).await {
            Ok(_) => {
                report(line(id, format!("{name} started.")));
                for extra in &service.extra_networks {
                    report(problem(
                        id,
                        format!(
                            "{name} was created on {} only; this engine attaches one network at a time, so {extra} was not joined.",
                            service.run.network.clone().unwrap_or_default()
                        ),
                    ));
                }
                Ok(())
            }
            Err(e) => Err(format!("{name} did not start: {}", e.message)),
        }
    }

    /// Every name the images on this engine answer to, tags and ids alike.
    async fn image_names(&self) -> BTreeSet<String> {
        let Ok(images) = self.images(false).await else {
            return BTreeSet::new();
        };
        images
            .iter()
            .flat_map(|i| i.repo_tags.iter().cloned().chain(std::iter::once(i.id.clone())))
            .collect()
    }

    /// Stop and remove a stack's containers, and the networks it owns.
    ///
    /// Driven by the project label rather than the file, so a stack can be
    /// taken down after the compose file it came from has been deleted.
    pub async fn compose_down(&self, project: &str, volumes: bool, report: Report<'_>) -> String {
        let id = project.to_string();
        let containers = match self.containers(true).await {
            Ok(list) => list,
            Err(e) => {
                let summary = format!("Could not list containers: {}", e.message);
                report(finished(&id, summary.clone(), Some(summary.clone())));
                return summary;
            }
        };

        let members: Vec<&Container> = containers
            .iter()
            .filter(|c| c.compose_project.as_deref() == Some(project))
            .collect();

        if members.is_empty() {
            let summary = format!("Nothing belonging to {project} is here.");
            report(finished(&id, summary.clone(), None));
            return summary;
        }

        let mut removed = 0usize;
        let mut failed = 0usize;
        for c in &members {
            let _ = self.container_stop(&c.id).await;
            match self.container_remove(&c.id, true, volumes).await {
                Ok(()) => {
                    removed += 1;
                    report(line(&id, format!("{} removed.", c.name)));
                }
                Err(e) => {
                    failed += 1;
                    report(problem(&id, format!("{} was not removed: {}", c.name, e.message)));
                }
            }
        }

        // Only networks this project created. An external one belongs to
        // someone else, and Hopper never made it.
        let scoped = format!("{project}_");
        if let Ok(networks) = self.networks().await {
            for net in networks.iter().filter(|n| n.name.starts_with(&scoped)) {
                match self.network_remove(&net.id).await {
                    Ok(()) => report(line(&id, format!("Network {} removed.", net.name))),
                    Err(e) => report(problem(
                        &id,
                        format!("Network {} was not removed: {}", net.name, e.message),
                    )),
                }
            }
        }

        if volumes {
            if let Ok(list) = self.volumes().await {
                for v in list.iter().filter(|v| v.name.starts_with(&scoped)) {
                    match self.volume_remove(&v.name, true).await {
                        Ok(()) => report(line(&id, format!("Volume {} removed.", v.name))),
                        Err(e) => report(problem(
                            &id,
                            format!("Volume {} was not removed: {}", v.name, e.message),
                        )),
                    }
                }
            }
        }

        let summary = if failed > 0 {
            format!("Removed {removed} of {}.", members.len())
        } else {
            format!("Removed {removed} {}.", plural(removed, "container", "containers"))
        };
        report(finished(&id, summary.clone(), (failed > 0).then(|| summary.clone())));
        summary
    }
}

/// Whether a container still matches the service it was created from.
///
/// The hash covers everything about the container that the file decides, so a
/// match means recreating it would produce the same thing. A container with no
/// hash was not made by Hopper and is always replaced.
fn unchanged(existing: &Container, service: &ComposePlanService) -> bool {
    let planned = service.run.labels.get(compose::names::CONFIG_HASH);
    let current = existing.labels.get(compose::names::CONFIG_HASH);
    match (planned, current) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// Whether an image is on the engine already, by any name it answers to.
fn have_image(present: &BTreeSet<String>, reference: &str) -> bool {
    present.contains(reference)
        // `nginx` means `nginx:latest` everywhere but in the tag list.
        || (!reference.contains(':') && present.contains(&format!("{reference}:latest")))
}

/// Record an image just pulled, under both spellings of a bare name, so a
/// later service asking for `nginx:latest` does not re-pull `nginx`.
fn remember_image(present: &mut BTreeSet<String>, reference: &str) {
    present.insert(reference.to_string());
    if !reference.contains(':') {
        present.insert(format!("{reference}:latest"));
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        one.to_string()
    } else {
        many.to_string()
    }
}

/// One line describing how the run went.
pub fn summarize(started: usize, failed: usize, skipped: usize) -> String {
    if started == 0 && failed == 0 {
        return "Nothing to start.".into();
    }
    let mut out = format!(
        "Started {started} {}",
        plural(started, "service", "services")
    );
    if failed > 0 {
        out.push_str(&format!(", {failed} failed"));
    }
    if skipped > 0 {
        out.push_str(&format!(", {skipped} skipped"));
    }
    out.push('.');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clean_run_reads_as_a_plain_count() {
        assert_eq!(summarize(3, 0, 0), "Started 3 services.");
        assert_eq!(summarize(1, 0, 0), "Started 1 service.");
    }

    #[test]
    fn failures_and_skips_are_never_hidden_behind_the_successes() {
        assert_eq!(summarize(2, 1, 0), "Started 2 services, 1 failed.");
        assert_eq!(summarize(2, 0, 1), "Started 2 services, 1 skipped.");
        assert_eq!(summarize(2, 1, 3), "Started 2 services, 1 failed, 3 skipped.");
    }

    #[test]
    fn an_empty_run_says_so_rather_than_claiming_success() {
        assert_eq!(summarize(0, 0, 0), "Nothing to start.");
    }

    fn labelled(name: &str, hash: Option<&str>, state: ContainerState) -> Container {
        let mut labels = std::collections::BTreeMap::new();
        if let Some(h) = hash {
            labels.insert(compose::names::CONFIG_HASH.to_string(), h.to_string());
        }
        Container {
            id: format!("id-{name}"),
            name: name.into(),
            image: "nginx".into(),
            image_id: String::new(),
            command: String::new(),
            created: 0,
            state,
            status: String::new(),
            health: model::Health::None,
            ports: vec![],
            labels,
            mounts: vec![],
            networks: vec![],
            compose_project: None,
            compose_service: None,
        }
    }

    fn planned(hash: &str) -> ComposePlanService {
        let mut run = model::RunInput::default();
        run.labels
            .insert(compose::names::CONFIG_HASH.to_string(), hash.to_string());
        ComposePlanService {
            service: "web".into(),
            run,
            ..Default::default()
        }
    }

    #[test]
    fn a_running_container_that_still_matches_the_file_is_left_alone() {
        // The whole point: `up` on an unchanged stack must not destroy it.
        let c = labelled("shop-web-1", Some("abc"), ContainerState::Running);
        assert!(unchanged(&c, &planned("abc")));
    }

    #[test]
    fn an_edited_service_no_longer_matches_its_container() {
        let c = labelled("shop-web-1", Some("abc"), ContainerState::Running);
        assert!(!unchanged(&c, &planned("def")));
    }

    #[test]
    fn a_container_hopper_did_not_create_is_always_replaced() {
        // No hash means nothing can be concluded, and reusing it would be a
        // guess about a container someone else made.
        let c = labelled("shop-web-1", None, ContainerState::Running);
        assert!(!unchanged(&c, &planned("abc")));
    }

    fn on_engine(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_bare_image_name_matches_the_latest_tag_it_really_carries() {
        // `image: nginx` in a compose file is `nginx:latest` in the tag list.
        let present = on_engine(&["nginx:latest", "postgres:16"]);
        assert!(have_image(&present, "nginx"));
        assert!(have_image(&present, "nginx:latest"));
        assert!(have_image(&present, "postgres:16"));
    }

    #[test]
    fn a_tag_the_engine_does_not_have_is_pulled_rather_than_assumed() {
        // The bare-name rule must not make `postgres:15` look present because
        // `postgres:16` is.
        let present = on_engine(&["postgres:16"]);
        assert!(!have_image(&present, "postgres:15"));
        assert!(!have_image(&present, "redis"));
    }

    #[test]
    fn an_image_pinned_by_id_is_recognized_without_a_tag() {
        let present = on_engine(&["sha256:abc123"]);
        assert!(have_image(&present, "sha256:abc123"));
    }

    #[test]
    fn a_pull_satisfies_a_later_service_that_spells_the_tag_differently() {
        // Two services on the same image, one written `nginx` and one
        // `nginx:latest`, must cost one pull rather than two.
        let mut present = BTreeSet::new();
        remember_image(&mut present, "nginx");
        assert!(have_image(&present, "nginx"));
        assert!(have_image(&present, "nginx:latest"));
    }
}
