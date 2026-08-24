//! Compose, implemented rather than shelled out to.
//!
//! Hopper does not run `docker compose`. On macOS the engine is Apple's
//! runtime, which publishes no Docker socket — so there is nothing for the
//! real Compose to talk to, and the binary itself is part of the Docker
//! install people came here to remove. A compose file is what most projects
//! *are*, so reading one has to work with no Docker on the machine at all.
//!
//! The crate is the front half of that: read the file, expand its variables,
//! resolve every service down to a container, and order them. It does no I/O
//! beyond reading the files it is handed, and it never talks to an engine —
//! `host` takes the plan and drives whichever backend is answering.
//!
//! Every gap is reported. A key Hopper does not implement becomes a warning on
//! the service it was written on, and a service that cannot run at all comes
//! back `blocked` with the reason. A stack that quietly comes up missing half
//! of what the file asked for is the one outcome worth engineering against.

pub mod file;
pub mod interpolate;
pub mod names;
pub mod parse;
pub mod plan;

pub use parse::{discover_in, host_vars, load, Loaded};
pub use plan::{build, PlanOptions};

use model::{ComposePlan, EngineCapabilities};
use std::path::Path;

/// Read a compose file set and plan it in one call.
pub fn plan_files(
    paths: &[String],
    env_file: Option<&str>,
    opts: &PlanOptions,
    caps: &EngineCapabilities,
) -> Result<ComposePlan, String> {
    let loaded = load(paths, env_file, &host_vars())?;
    build(&loaded, opts, caps)
}

/// Read whatever compose file a directory holds, and plan it.
pub fn plan_dir(
    dir: &Path,
    opts: &PlanOptions,
    caps: &EngineCapabilities,
) -> Result<ComposePlan, String> {
    let found = discover_in(dir);
    if found.is_empty() {
        return Err(format!(
            "No compose file in {}. Hopper looks for {}.",
            dir.display(),
            parse::CANDIDATES.join(", ")
        ));
    }
    let paths: Vec<String> = found
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    plan_files(&paths, None, opts, caps)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_directory_with_no_compose_file_says_what_it_looked_for() {
        let dir = std::env::temp_dir().join("hopper-compose-lib-empty");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let err = plan_dir(&dir, &PlanOptions::default(), &EngineCapabilities::apple()).unwrap_err();
        assert!(err.contains("compose.yaml"));
        assert!(err.contains("docker-compose.yml"));
    }

    #[test]
    fn a_real_file_plans_end_to_end() {
        let dir = std::env::temp_dir().join("hopper-compose-lib-real");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("compose.yaml"),
            "name: demo\nservices:\n  web:\n    image: nginx\n    ports: ['8080:80']\n    depends_on: [db]\n  db:\n    image: postgres\n    volumes:\n      - data:/var/lib/postgresql/data\nvolumes:\n  data:\n",
        )
        .unwrap();

        let plan = plan_dir(&dir, &PlanOptions::default(), &EngineCapabilities::apple()).unwrap();
        assert_eq!(plan.project, "demo");
        assert_eq!(plan.volumes[0].name, "demo_data");
        assert_eq!(plan.networks[0].name, "demo_default");
        let order: Vec<&str> = plan.services.iter().map(|s| s.service.as_str()).collect();
        assert_eq!(order, vec!["db", "web"], "a dependency starts first");
        assert_eq!(plan.runnable().len(), 2);
    }
}
