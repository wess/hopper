//! Reconstructing compose stacks from container labels.
//!
//! Grouping this way rather than shelling out to `docker compose ps` means
//! stacks appear against any engine, with no compose CLI installed, and
//! without a project directory on disk. It is pure over a container list, so
//! it tests without a daemon.

use compose::names::{CONFIG_FILES, WORKING_DIR};
use model::{ComposeProject, ComposeService, Container, ContainerState};
use std::collections::BTreeMap;

/// Group containers into the compose projects they belong to.
///
/// Containers with no compose project are not stacks and are skipped;
/// projects come back sorted so the list does not reshuffle between refreshes.
pub fn group(containers: &[Container]) -> Vec<ComposeProject> {
    let mut by_project: BTreeMap<String, Vec<&Container>> = BTreeMap::new();
    for c in containers {
        if let Some(project) = c.compose_project.as_ref().filter(|p| !p.is_empty()) {
            by_project.entry(project.clone()).or_default().push(c);
        }
    }

    by_project
        .into_iter()
        .map(|(name, members)| {
            let running = members
                .iter()
                .filter(|c| c.state == ContainerState::Running)
                .count();
            let total = members.len();

            // Config files and working dir come from the labels compose wrote;
            // any member carries them, so take the first that has them.
            let config_files = members
                .iter()
                .find_map(|c| c.labels.get(CONFIG_FILES))
                .map(|v| {
                    v.split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            let working_dir = members
                .iter()
                .find_map(|c| c.labels.get(WORKING_DIR))
                .cloned();

            let mut services: Vec<ComposeService> = members
                .iter()
                .map(|c| ComposeService {
                    service: c
                        .compose_service
                        .clone()
                        .unwrap_or_else(|| c.name.clone()),
                    container_id: c.id.clone(),
                    container_name: c.name.clone(),
                    state: c.state,
                    status: c.status.clone(),
                    image: c.image.clone(),
                    ports: c.ports.clone(),
                })
                .collect();
            // Stable order: by service, then container name for scaled replicas.
            services.sort_by(|a, b| {
                a.service
                    .cmp(&b.service)
                    .then_with(|| a.container_name.cmp(&b.container_name))
            });

            ComposeProject {
                name,
                status: ComposeProject::status_for(running, total),
                running,
                total,
                services,
                config_files,
                working_dir,
            }
        })
        .collect()
}

/// Whether a stack can be brought up from what we know about it. `up` needs
/// the compose file; the label only records where it was.
pub fn can_start_from_files(project: &ComposeProject) -> bool {
    project
        .config_files
        .iter()
        .any(|f| std::path::Path::new(f).exists())
}

#[cfg(test)]
mod tests {
    use super::*;
    use compose::names::PROJECT;
    use model::{ComposeStackStatus, Health};

    fn container(name: &str, project: Option<&str>, service: Option<&str>, state: ContainerState) -> Container {
        let mut labels = BTreeMap::new();
        if let Some(p) = project {
            labels.insert(PROJECT.to_string(), p.to_string());
            labels.insert(
                CONFIG_FILES.to_string(),
                "/srv/app/compose.yaml,/srv/app/compose.override.yaml".to_string(),
            );
            labels.insert(WORKING_DIR.to_string(), "/srv/app".to_string());
        }
        Container {
            id: format!("id-{name}"),
            name: name.into(),
            image: "img".into(),
            image_id: String::new(),
            command: String::new(),
            created: 0,
            state,
            status: "Up".into(),
            health: Health::None,
            ports: vec![],
            labels,
            mounts: vec![],
            networks: vec![],
            compose_project: project.map(str::to_string),
            compose_service: service.map(str::to_string),
        }
    }

    #[test]
    fn groups_containers_into_their_projects() {
        let list = vec![
            container("shop-web-1", Some("shop"), Some("web"), ContainerState::Running),
            container("shop-db-1", Some("shop"), Some("db"), ContainerState::Running),
            container("blog-web-1", Some("blog"), Some("web"), ContainerState::Exited),
        ];
        let projects = group(&list);
        assert_eq!(projects.len(), 2);
        // Sorted, so the list is stable across refreshes.
        assert_eq!(projects[0].name, "blog");
        assert_eq!(projects[1].name, "shop");
        assert_eq!(projects[1].total, 2);
        assert_eq!(projects[1].running, 2);
        assert_eq!(projects[1].status, ComposeStackStatus::Running);
    }

    #[test]
    fn a_partially_running_stack_is_marked_partial() {
        let list = vec![
            container("shop-web-1", Some("shop"), Some("web"), ContainerState::Running),
            container("shop-db-1", Some("shop"), Some("db"), ContainerState::Exited),
        ];
        let projects = group(&list);
        assert_eq!(projects[0].status, ComposeStackStatus::Partial);
        assert_eq!(projects[0].running, 1);
    }

    #[test]
    fn containers_outside_any_stack_are_ignored() {
        let list = vec![
            container("standalone", None, None, ContainerState::Running),
            container("shop-web-1", Some("shop"), Some("web"), ContainerState::Running),
        ];
        let projects = group(&list);
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "shop");
    }

    #[test]
    fn config_files_are_split_out_of_the_comma_joined_label() {
        let list = vec![container("shop-web-1", Some("shop"), Some("web"), ContainerState::Running)];
        let projects = group(&list);
        assert_eq!(projects[0].config_files.len(), 2);
        assert_eq!(projects[0].config_files[0], "/srv/app/compose.yaml");
        assert_eq!(projects[0].working_dir.as_deref(), Some("/srv/app"));
    }

    #[test]
    fn scaled_replicas_all_appear_under_their_service() {
        let list = vec![
            container("shop-web-2", Some("shop"), Some("web"), ContainerState::Running),
            container("shop-web-1", Some("shop"), Some("web"), ContainerState::Running),
        ];
        let projects = group(&list);
        assert_eq!(projects[0].services.len(), 2);
        // Replicas sort by container name so the rows do not jump around.
        assert_eq!(projects[0].services[0].container_name, "shop-web-1");
        assert_eq!(projects[0].service_names(), vec!["web".to_string()]);
    }

    #[test]
    fn a_service_without_a_service_label_falls_back_to_its_container_name() {
        let list = vec![container("odd-one", Some("shop"), None, ContainerState::Running)];
        let projects = group(&list);
        assert_eq!(projects[0].services[0].service, "odd-one");
    }

    #[test]
    fn an_empty_container_list_produces_no_stacks() {
        assert!(group(&[]).is_empty());
    }
}
