//! Finding compose files, reading them, and turning the text into a spec.
//!
//! The order matters and is compose's own: read `.env` from the project
//! directory, overlay the process environment, expand `${...}` in the raw
//! text, parse the YAML, then merge each override file over the base.

use crate::file::File;
use crate::interpolate::{self, Vars};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The file names compose resolves, most preferred first.
pub const CANDIDATES: [&str; 4] = [
    "compose.yaml",
    "compose.yml",
    "docker-compose.yaml",
    "docker-compose.yml",
];

/// The override files compose layers over the base, in the same order.
pub const OVERRIDES: [&str; 4] = [
    "compose.override.yaml",
    "compose.override.yml",
    "docker-compose.override.yaml",
    "docker-compose.override.yml",
];

/// A compose file set, read and ready to plan from.
#[derive(Clone, Debug, Default)]
pub struct Loaded {
    pub file: File,
    /// Where relative paths — build contexts, bind mounts, env files — resolve
    /// against. The first file's directory, as compose does it.
    pub project_dir: PathBuf,
    /// Every file that went into this, base first.
    pub config_files: Vec<String>,
    pub vars: Vars,
    /// Things the user should know that are not fatal.
    pub warnings: Vec<String>,
}

/// Find the compose file in a directory, plus its override sibling.
pub fn discover_in(dir: &Path) -> Vec<PathBuf> {
    let Some(base) = CANDIDATES.iter().map(|n| dir.join(n)).find(|p| p.is_file()) else {
        return Vec::new();
    };
    let mut out = vec![base];
    if let Some(over) = OVERRIDES.iter().map(|n| dir.join(n)).find(|p| p.is_file()) {
        out.push(over);
    }
    out
}

/// The process environment, as the variables a file expands against.
pub fn host_vars() -> Vars {
    std::env::vars().collect()
}

/// Read a file set and produce the spec.
///
/// `env` is normally [`host_vars`]; it is a parameter so the substitution
/// rules can be tested without touching the real environment.
pub fn load(paths: &[String], env_file: Option<&str>, env: &Vars) -> Result<Loaded, String> {
    let first = paths
        .iter()
        .find(|p| !p.trim().is_empty())
        .ok_or_else(|| "No compose file was given.".to_string())?;
    let project_dir = Path::new(first)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));

    let mut warnings = Vec::new();

    // `.env` first, process environment over the top — an exported variable
    // beats the file, which is what people rely on for one-off overrides.
    let mut vars: Vars = BTreeMap::new();
    let dotenv = match env_file {
        Some(path) => PathBuf::from(path),
        None => project_dir.join(".env"),
    };
    match std::fs::read_to_string(&dotenv) {
        Ok(text) => vars.extend(interpolate::parse_env(&text)),
        Err(e) if env_file.is_some() => {
            return Err(format!("Could not read {}: {e}", dotenv.display()));
        }
        // No `.env` is the normal case, not a problem.
        Err(_) => {}
    }
    vars.extend(env.clone());

    let mut merged: Option<serde_yaml::Value> = None;
    let mut config_files = Vec::new();

    for path in paths.iter().filter(|p| !p.trim().is_empty()) {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("Could not read {path}: {e}"))?;
        let (expanded, unresolved) = interpolate::expand(&text, &vars);

        if !unresolved.required.is_empty() {
            let (name, message) = &unresolved.required[0];
            return Err(if message.is_empty() {
                format!("{path} requires ${name}, which is not set.")
            } else {
                format!("{path} requires ${name}: {message}")
            });
        }
        let missing = unresolved.missing.clone();
        for name in &missing {
            warnings.push(format!(
                "${name} is not set and has no default, so it expanded to nothing in {path}."
            ));
        }

        // An unset variable does not usually break the parse — it just leaves a
        // blank. But `image: app:${TAG}` collapses to `image: app:`, which YAML
        // reads as a nested mapping, and blaming the YAML would send the user
        // looking at the wrong line. The variable is the cause; say so.
        let value: serde_yaml::Value = serde_yaml::from_str(&expanded).map_err(|e| {
            if missing.is_empty() {
                format!("{path} is not valid YAML: {e}")
            } else {
                format!(
                    "{path} did not survive substitution: {} is not set, and expanding it to nothing left the file unparseable ({e}). Set it, or give it a default with ${{{}:-value}}.",
                    missing
                        .iter()
                        .map(|n| format!("${n}"))
                        .collect::<Vec<_>>()
                        .join(", "),
                    missing[0]
                )
            }
        })?;
        merged = Some(match merged {
            None => value,
            Some(base) => merge(base, value),
        });
        config_files.push(path.clone());
    }

    let value = merged.unwrap_or(serde_yaml::Value::Null);
    // An empty file parses to null, which is not an error but is not a stack.
    if value.is_null() {
        return Err(format!("{first} has nothing in it."));
    }
    let file: File = serde_yaml::from_value(value)
        .map_err(|e| format!("{first} is not a compose file: {e}"))?;

    if file.services.is_empty() {
        return Err(format!("{first} declares no services."));
    }

    Ok(Loaded {
        file,
        project_dir,
        config_files,
        vars,
        warnings,
    })
}

/// Layer an override file over a base.
///
/// Maps merge key by key; anything else is replaced outright. That is the
/// common case — an override file changes a scalar or adds a service — and it
/// is the behaviour that never silently *combines* two lists into something
/// neither file asked for.
fn merge(base: serde_yaml::Value, over: serde_yaml::Value) -> serde_yaml::Value {
    match (base, over) {
        (serde_yaml::Value::Mapping(mut a), serde_yaml::Value::Mapping(b)) => {
            for (k, v) in b {
                let next = match a.remove(&k) {
                    Some(existing) => merge(existing, v),
                    None => v,
                };
                a.insert(k, next);
            }
            serde_yaml::Value::Mapping(a)
        }
        (_, over) => over,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hopper-compose-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(dir: &Path, name: &str, text: &str) -> String {
        let p = dir.join(name);
        std::fs::write(&p, text).unwrap();
        p.to_string_lossy().to_string()
    }

    #[test]
    fn discovery_prefers_the_modern_name_and_picks_up_an_override() {
        let dir = scratch("discover");
        write(&dir, "docker-compose.yml", "services: {}");
        write(&dir, "compose.yaml", "services: {}");
        write(&dir, "compose.override.yaml", "services: {}");
        let found = discover_in(&dir);
        assert_eq!(found.len(), 2);
        assert!(found[0].ends_with("compose.yaml"));
        assert!(found[1].ends_with("compose.override.yaml"));
    }

    #[test]
    fn a_directory_with_no_compose_file_finds_nothing() {
        assert!(discover_in(&scratch("empty")).is_empty());
    }

    #[test]
    fn a_file_loads_with_its_services() {
        let dir = scratch("basic");
        let p = write(&dir, "compose.yaml", "services:\n  web:\n    image: nginx\n");
        let loaded = load(&[p], None, &Vars::new()).unwrap();
        assert!(loaded.file.services.contains_key("web"));
        assert_eq!(loaded.project_dir, dir);
    }

    #[test]
    fn an_override_file_wins_key_by_key() {
        let dir = scratch("override");
        let base = write(
            &dir,
            "compose.yaml",
            "services:\n  web:\n    image: nginx\n    user: root\n",
        );
        let over = write(&dir, "compose.override.yaml", "services:\n  web:\n    image: caddy\n");
        let loaded = load(&[base, over], None, &Vars::new()).unwrap();
        let web = &loaded.file.services["web"];
        assert_eq!(web.image.as_deref(), Some("caddy"));
        // The key the override did not mention survives.
        assert_eq!(web.user.as_deref(), Some("root"));
    }

    #[test]
    fn a_dot_env_file_supplies_variables() {
        let dir = scratch("dotenv");
        std::fs::write(dir.join(".env"), "TAG=1.4\n").unwrap();
        let p = write(&dir, "compose.yaml", "services:\n  web:\n    image: app:${TAG}\n");
        let loaded = load(&[p], None, &Vars::new()).unwrap();
        assert_eq!(loaded.file.services["web"].image.as_deref(), Some("app:1.4"));
    }

    #[test]
    fn the_process_environment_beats_the_dot_env_file() {
        let dir = scratch("envwins");
        std::fs::write(dir.join(".env"), "TAG=fromfile\n").unwrap();
        let p = write(&dir, "compose.yaml", "services:\n  web:\n    image: app:${TAG}\n");
        let env: Vars = [("TAG".to_string(), "fromenv".to_string())].into();
        let loaded = load(&[p], None, &env).unwrap();
        assert_eq!(
            loaded.file.services["web"].image.as_deref(),
            Some("app:fromenv")
        );
    }

    #[test]
    fn an_unset_variable_warns_rather_than_failing() {
        let dir = scratch("unset");
        // Quoted, so expanding to nothing leaves the file parseable.
        let p = write(&dir, "compose.yaml", "services:\n  web:\n    image: \"app:${TAG}\"\n");
        let loaded = load(&[p], None, &Vars::new()).unwrap();
        assert!(loaded.warnings.iter().any(|w| w.contains("$TAG")));
        assert_eq!(loaded.file.services["web"].image.as_deref(), Some("app:"));
    }

    #[test]
    fn an_unset_variable_that_breaks_the_parse_blames_the_variable_not_the_yaml() {
        // `image: app:${TAG}` collapses to `image: app:`, which YAML reads as a
        // nested mapping. Reporting a YAML error here sends the user to the
        // wrong line entirely.
        let dir = scratch("unsetbreaks");
        let p = write(&dir, "compose.yaml", "services:\n  web:\n    image: app:${TAG}\n");
        let err = load(&[p], None, &Vars::new()).unwrap_err();
        assert!(err.contains("$TAG"), "{err}");
        assert!(err.contains("${TAG:-value}"), "and suggests the fix: {err}");
    }

    #[test]
    fn a_required_variable_stops_the_load_with_the_files_own_message() {
        let dir = scratch("required");
        let p = write(
            &dir,
            "compose.yaml",
            "services:\n  db:\n    image: postgres\n    environment:\n      P: ${DB_PASSWORD:?set a password}\n",
        );
        let err = load(&[p], None, &Vars::new()).unwrap_err();
        assert!(err.contains("DB_PASSWORD"));
        assert!(err.contains("set a password"));
    }

    #[test]
    fn broken_yaml_is_refused_by_name() {
        let dir = scratch("broken");
        let p = write(&dir, "compose.yaml", "services:\n  web:\n   - bad\n  indent\n");
        let err = load(&[p], None, &Vars::new()).unwrap_err();
        assert!(err.contains("compose.yaml"));
    }

    #[test]
    fn a_file_with_no_services_is_refused_rather_than_planned_as_empty() {
        let dir = scratch("noservices");
        let p = write(&dir, "compose.yaml", "networks:\n  app:\n");
        assert!(load(&[p], None, &Vars::new()).unwrap_err().contains("no services"));
    }

    #[test]
    fn a_named_env_file_that_is_missing_is_an_error_unlike_a_missing_dot_env() {
        let dir = scratch("envfile");
        let p = write(&dir, "compose.yaml", "services:\n  web:\n    image: nginx\n");
        // Asking for one by name and not getting it is a mistake worth stopping for.
        assert!(load(std::slice::from_ref(&p), Some("/nonexistent/.env"), &Vars::new()).is_err());
        // Not having a `.env` at all is the normal case.
        assert!(load(&[p], None, &Vars::new()).is_ok());
    }
}
