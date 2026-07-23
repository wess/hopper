//! Workspaces — saved, named scopes over your Docker resources.
//!
//! Pick the compose projects and/or a name pattern you care about and the UI
//! filters to just those. The built-in "all" workspace (no predicates) shows
//! everything. A container matches when *every* provided predicate matches.

use super::container::Container;
use regex::RegexBuilder;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub id: String,
    pub name: String,
    /// The container's compose project must be one of these.
    #[serde(default)]
    pub compose_projects: Vec<String>,
    /// Regex tested against the container/image name.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub name_pattern: Option<String>,
}

/// Compile a user-supplied pattern. An invalid regex makes the predicate
/// absent rather than matching nothing — a half-typed pattern in the settings
/// field must not blank every view.
fn safe_regex(pattern: &str) -> Option<regex::Regex> {
    RegexBuilder::new(pattern).case_insensitive(true).build().ok()
}

impl Workspace {
    fn has_pattern(&self) -> Option<&str> {
        self.name_pattern.as_deref().filter(|p| !p.trim().is_empty())
    }

    /// Whether a container falls inside this scope.
    pub fn matches(&self, c: &Container) -> bool {
        if !self.compose_projects.is_empty() {
            match &c.compose_project {
                Some(project) if self.compose_projects.iter().any(|p| p == project) => {}
                _ => return false,
            }
        }
        if let Some(pattern) = self.has_pattern() {
            if let Some(re) = safe_regex(pattern) {
                if !re.is_match(&c.name) && !re.is_match(&c.image) {
                    return false;
                }
            }
        }
        true
    }

    /// Images carry no compose project, so only the name pattern scopes them.
    pub fn matches_image(&self, repo_tags: &[String]) -> bool {
        let Some(pattern) = self.has_pattern() else {
            return true;
        };
        let Some(re) = safe_regex(pattern) else {
            return true;
        };
        repo_tags.iter().any(|t| re.is_match(t))
    }
}

/// A `None` workspace is the built-in "all" scope and matches everything.
pub fn matches_workspace(c: &Container, ws: Option<&Workspace>) -> bool {
    ws.map_or(true, |w| w.matches(c))
}

pub fn image_matches_workspace(repo_tags: &[String], ws: Option<&Workspace>) -> bool {
    ws.map_or(true, |w| w.matches_image(repo_tags))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::{ContainerState, Health};
    use std::collections::BTreeMap;

    fn container(name: &str, image: &str, project: Option<&str>) -> Container {
        Container {
            id: "id".into(),
            name: name.into(),
            image: image.into(),
            image_id: String::new(),
            command: String::new(),
            created: 0,
            state: ContainerState::Running,
            status: String::new(),
            health: Health::None,
            ports: vec![],
            labels: BTreeMap::new(),
            mounts: vec![],
            networks: vec![],
            compose_project: project.map(str::to_string),
            compose_service: None,
        }
    }

    #[test]
    fn no_workspace_matches_everything() {
        let c = container("web", "nginx", None);
        assert!(matches_workspace(&c, None));
        assert!(image_matches_workspace(&["nginx:latest".into()], None));
    }

    #[test]
    fn compose_project_predicate_scopes_by_project() {
        let ws = Workspace {
            compose_projects: vec!["shop".into()],
            ..Default::default()
        };
        assert!(ws.matches(&container("web", "nginx", Some("shop"))));
        assert!(!ws.matches(&container("web", "nginx", Some("blog"))));
        // A container outside any stack cannot match a project predicate.
        assert!(!ws.matches(&container("web", "nginx", None)));
    }

    #[test]
    fn name_pattern_tests_name_and_image_case_insensitively() {
        let ws = Workspace {
            name_pattern: Some("NGIN".into()),
            ..Default::default()
        };
        assert!(ws.matches(&container("web", "nginx:latest", None)));
        assert!(ws.matches(&container("nginx-proxy", "other", None)));
        assert!(!ws.matches(&container("api", "postgres", None)));
    }

    #[test]
    fn predicates_combine_with_and() {
        let ws = Workspace {
            compose_projects: vec!["shop".into()],
            name_pattern: Some("web".into()),
            ..Default::default()
        };
        assert!(ws.matches(&container("web", "nginx", Some("shop"))));
        // Right project, wrong name.
        assert!(!ws.matches(&container("db", "postgres", Some("shop"))));
        // Right name, wrong project.
        assert!(!ws.matches(&container("web", "nginx", Some("blog"))));
    }

    #[test]
    fn an_invalid_pattern_is_treated_as_absent() {
        let ws = Workspace {
            name_pattern: Some("([unclosed".into()),
            ..Default::default()
        };
        assert!(ws.matches(&container("anything", "any", None)));
        assert!(ws.matches_image(&["any:tag".into()]));
    }

    #[test]
    fn a_blank_pattern_is_ignored() {
        let ws = Workspace {
            name_pattern: Some("   ".into()),
            ..Default::default()
        };
        assert!(ws.matches(&container("anything", "any", None)));
    }

    #[test]
    fn images_are_scoped_by_pattern_only() {
        let ws = Workspace {
            compose_projects: vec!["shop".into()],
            name_pattern: Some("nginx".into()),
            ..Default::default()
        };
        assert!(ws.matches_image(&["nginx:1.25".into()]));
        assert!(!ws.matches_image(&["postgres:16".into()]));
    }
}
