//! Reading and writing compose files on the host, for the in-app editor.

use crate::error::Result;
use model::{ComposeFileResult, ComposeTarget};
use std::path::{Path, PathBuf};

/// The conventional file names, in the order Compose itself resolves them.
pub const CANDIDATES: [&str; 4] = [
    "compose.yaml",
    "compose.yml",
    "docker-compose.yaml",
    "docker-compose.yml",
];

/// The directory a Compose command should run in.
///
/// Compose resolves relative build contexts and env files against its working
/// directory, so running from the wrong place silently changes what gets
/// built. The first file's parent is the right answer.
pub fn working_dir(target: &ComposeTarget) -> Option<String> {
    target
        .files
        .iter()
        .find(|f| !f.trim().is_empty())
        .and_then(|f| Path::new(f).parent())
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.to_string_lossy().to_string())
}

/// Find a compose file in a directory.
pub fn discover_in(dir: &Path) -> Option<PathBuf> {
    CANDIDATES
        .iter()
        .map(|name| dir.join(name))
        .find(|p| p.is_file())
}

/// Read a compose file for the editor.
pub fn read(path: &str) -> ComposeFileResult {
    match std::fs::read_to_string(path) {
        Ok(content) => ComposeFileResult {
            ok: true,
            content: Some(content),
            error: None,
        },
        Err(e) => ComposeFileResult {
            ok: false,
            content: None,
            error: Some(format!("Could not read {path}: {e}")),
        },
    }
}

/// Write a compose file back, refusing content that is not valid YAML.
///
/// Saving a syntactically broken file would leave the user with a stack that
/// cannot start and an editor that reported success.
pub fn write(path: &str, content: &str) -> ComposeFileResult {
    if let Err(e) = serde_yaml::from_str::<serde_yaml::Value>(content) {
        return ComposeFileResult {
            ok: false,
            content: None,
            error: Some(format!("That is not valid YAML: {e}")),
        };
    }
    match std::fs::write(path, content) {
        Ok(()) => ComposeFileResult {
            ok: true,
            content: Some(content.to_string()),
            error: None,
        },
        Err(e) => ComposeFileResult {
            ok: false,
            content: None,
            error: Some(format!("Could not write {path}: {e}")),
        },
    }
}

/// Which of a project's recorded config files still exist on disk.
pub fn existing(files: &[String]) -> Vec<String> {
    files
        .iter()
        .filter(|f| Path::new(f).is_file())
        .cloned()
        .collect()
}

/// Parse the service names out of a compose document, for the scale UI.
pub fn service_names(yaml: &str) -> Result<Vec<String>> {
    let doc: serde_yaml::Value = serde_yaml::from_str(yaml).unwrap_or(serde_yaml::Value::Null);
    let Some(services) = doc.get("services").and_then(|s| s.as_mapping()) else {
        return Ok(Vec::new());
    };
    Ok(services
        .keys()
        .filter_map(|k| k.as_str().map(str::to_string))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_working_directory_is_the_first_files_parent() {
        let target = ComposeTarget {
            files: vec!["/srv/app/compose.yaml".into()],
            ..Default::default()
        };
        assert_eq!(working_dir(&target).as_deref(), Some("/srv/app"));
    }

    #[test]
    fn a_bare_filename_has_no_working_directory_to_impose() {
        let target = ComposeTarget {
            files: vec!["compose.yaml".into()],
            ..Default::default()
        };
        assert_eq!(working_dir(&target), None);
    }

    #[test]
    fn no_files_means_no_working_directory() {
        assert_eq!(working_dir(&ComposeTarget::default()), None);
    }

    #[test]
    fn blank_entries_are_skipped_when_choosing_a_working_directory() {
        let target = ComposeTarget {
            files: vec!["  ".into(), "/srv/other/compose.yaml".into()],
            ..Default::default()
        };
        assert_eq!(working_dir(&target).as_deref(), Some("/srv/other"));
    }

    #[test]
    fn service_names_come_out_of_a_document() {
        let yaml = "services:\n  web:\n    image: nginx\n  db:\n    image: postgres\n";
        let mut names = service_names(yaml).unwrap();
        names.sort();
        assert_eq!(names, vec!["db", "web"]);
    }

    #[test]
    fn a_document_without_services_yields_nothing_rather_than_failing() {
        assert!(service_names("version: '3'\n").unwrap().is_empty());
        assert!(service_names("not: [valid").unwrap().is_empty());
        assert!(service_names("").unwrap().is_empty());
    }

    #[test]
    fn saving_invalid_yaml_is_refused_before_it_reaches_disk() {
        let path = std::env::temp_dir().join("hopper-invalid-compose.yaml");
        let _ = std::fs::remove_file(&path);
        let result = write(path.to_str().unwrap(), "services:\n  web:\n   - broken: [");
        assert!(!result.ok);
        assert!(result.error.unwrap().contains("valid YAML"));
        assert!(
            !path.exists(),
            "a rejected document must not have been written"
        );
    }

    #[test]
    fn a_valid_document_round_trips_through_disk() {
        let path = std::env::temp_dir().join("hopper-valid-compose.yaml");
        let content = "services:\n  web:\n    image: nginx\n";
        assert!(write(path.to_str().unwrap(), content).ok);
        let read_back = read(path.to_str().unwrap());
        assert!(read_back.ok);
        assert_eq!(read_back.content.as_deref(), Some(content));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn reading_a_missing_file_reports_why() {
        let result = read("/nonexistent/hopper/compose.yaml");
        assert!(!result.ok);
        assert!(result.error.unwrap().contains("Could not read"));
    }

    #[test]
    fn existing_filters_out_files_that_are_gone() {
        let real = std::env::temp_dir().join("hopper-exists.yaml");
        std::fs::write(&real, "services: {}\n").unwrap();
        let files = vec![
            real.to_string_lossy().to_string(),
            "/nonexistent/gone.yaml".to_string(),
        ];
        assert_eq!(existing(&files).len(), 1);
        let _ = std::fs::remove_file(&real);
    }
}
