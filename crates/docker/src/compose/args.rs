//! Building the `docker compose` argument vector.
//!
//! Shelling out to Compose rather than reimplementing the spec is deliberate:
//! the file format has years of accumulated behavior (profiles, extends,
//! interpolation, merge order) that would be a liability to reproduce. That
//! makes the argv the whole contract, so it is pure and heavily tested.

use model::{ComposeAction, ComposeOptions, ComposeTarget};

/// Global flags that come *before* the subcommand.
fn global(target: &ComposeTarget) -> Vec<String> {
    let mut args = Vec::new();
    for file in &target.files {
        if !file.trim().is_empty() {
            args.push("-f".into());
            args.push(file.clone());
        }
    }
    if let Some(project) = target.project.as_deref().filter(|p| !p.trim().is_empty()) {
        args.push("-p".into());
        args.push(project.into());
    }
    if let Some(env) = target.env_file.as_deref().filter(|e| !e.trim().is_empty()) {
        args.push("--env-file".into());
        args.push(env.into());
    }
    args
}

/// The full argument vector for an action.
pub fn build(action: ComposeAction, target: &ComposeTarget, opts: &ComposeOptions) -> Vec<String> {
    let mut args = global(target);

    for profile in &opts.profiles {
        if !profile.trim().is_empty() {
            args.push("--profile".into());
            args.push(profile.clone());
        }
    }

    match action {
        ComposeAction::Up => {
            args.push("up".into());
            // Detached: the UI streams the run's output itself and must not
            // inherit a foreground process that never exits.
            args.push("-d".into());
            if opts.build {
                args.push("--build".into());
            }
            if opts.force_recreate {
                args.push("--force-recreate".into());
            }
            // `up` defaults to cleaning up orphans; the others do not.
            if opts.remove_orphans.unwrap_or(true) {
                args.push("--remove-orphans".into());
            }
            for scale in &opts.scale {
                args.push("--scale".into());
                args.push(format!("{}={}", scale.service, scale.count));
            }
        }
        ComposeAction::Down => {
            args.push("down".into());
            if opts.volumes {
                args.push("--volumes".into());
            }
            if let Some(rmi) = opts.rmi.as_deref().filter(|r| !r.is_empty()) {
                args.push("--rmi".into());
                args.push(rmi.into());
            }
            if opts.remove_orphans.unwrap_or(false) {
                args.push("--remove-orphans".into());
            }
        }
        ComposeAction::Remove => {
            // A full teardown: containers, volumes, images, and orphans.
            args.push("down".into());
            args.push("--volumes".into());
            args.push("--remove-orphans".into());
            args.push("--rmi".into());
            args.push(opts.rmi.clone().unwrap_or_else(|| "local".into()));
        }
        ComposeAction::Start => args.push("start".into()),
        ComposeAction::Stop => args.push("stop".into()),
        ComposeAction::Restart => args.push("restart".into()),
        ComposeAction::Pull => args.push("pull".into()),
        ComposeAction::Build => args.push("build".into()),
    }

    // Service scoping goes last, after the subcommand's own flags.
    // `down` takes no service list — it tears down the project.
    if !matches!(action, ComposeAction::Down | ComposeAction::Remove) {
        for service in &opts.services {
            if !service.trim().is_empty() {
                args.push(service.clone());
            }
        }
    }

    args
}

/// The argv for `docker compose config`, used to validate a file set.
pub fn config(target: &ComposeTarget) -> Vec<String> {
    let mut args = global(target);
    args.push("config".into());
    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::ComposeScale;

    fn target() -> ComposeTarget {
        ComposeTarget {
            files: vec!["/srv/app/compose.yaml".into()],
            project: Some("shop".into()),
            env_file: None,
        }
    }

    #[test]
    fn up_is_detached_and_removes_orphans_by_default() {
        let args = build(ComposeAction::Up, &target(), &ComposeOptions::default());
        assert_eq!(
            args,
            vec![
                "-f",
                "/srv/app/compose.yaml",
                "-p",
                "shop",
                "up",
                "-d",
                "--remove-orphans"
            ]
        );
    }

    #[test]
    fn up_can_opt_out_of_removing_orphans() {
        let opts = ComposeOptions {
            remove_orphans: Some(false),
            ..Default::default()
        };
        let args = build(ComposeAction::Up, &target(), &opts);
        assert!(!args.contains(&"--remove-orphans".to_string()));
    }

    #[test]
    fn down_does_not_remove_orphans_unless_asked() {
        let args = build(ComposeAction::Down, &target(), &ComposeOptions::default());
        assert!(args.contains(&"down".to_string()));
        assert!(!args.contains(&"--remove-orphans".to_string()));
        assert!(!args.contains(&"--volumes".to_string()));
    }

    #[test]
    fn remove_is_a_full_teardown() {
        let args = build(ComposeAction::Remove, &target(), &ComposeOptions::default());
        assert!(args.contains(&"down".to_string()));
        assert!(args.contains(&"--volumes".to_string()));
        assert!(args.contains(&"--remove-orphans".to_string()));
        assert!(args.contains(&"--rmi".to_string()));
        assert!(args.contains(&"local".to_string()));
    }

    #[test]
    fn multiple_files_are_each_passed_with_their_own_flag() {
        let target = ComposeTarget {
            files: vec!["a.yaml".into(), "b.yaml".into()],
            ..Default::default()
        };
        let args = build(ComposeAction::Up, &target, &ComposeOptions::default());
        assert_eq!(
            args.iter().filter(|a| *a == "-f").count(),
            2,
            "override files must all be passed, in order"
        );
        assert_eq!(args[1], "a.yaml");
        assert_eq!(args[3], "b.yaml");
    }

    #[test]
    fn blank_files_and_projects_are_skipped() {
        let target = ComposeTarget {
            files: vec!["  ".into()],
            project: Some(String::new()),
            env_file: Some("   ".into()),
        };
        let args = build(ComposeAction::Stop, &target, &ComposeOptions::default());
        assert_eq!(args, vec!["stop"]);
    }

    #[test]
    fn scale_pairs_are_rendered_for_up() {
        let opts = ComposeOptions {
            scale: vec![
                ComposeScale {
                    service: "web".into(),
                    count: 3,
                },
                ComposeScale {
                    service: "worker".into(),
                    count: 2,
                },
            ],
            ..Default::default()
        };
        let args = build(ComposeAction::Up, &target(), &opts);
        assert!(args.contains(&"web=3".to_string()));
        assert!(args.contains(&"worker=2".to_string()));
        assert_eq!(args.iter().filter(|a| *a == "--scale").count(), 2);
    }

    #[test]
    fn services_scope_lifecycle_actions() {
        let opts = ComposeOptions {
            services: vec!["web".into()],
            ..Default::default()
        };
        let args = build(ComposeAction::Restart, &target(), &opts);
        assert_eq!(args.last().unwrap(), "web");
    }

    #[test]
    fn down_ignores_a_service_list_because_it_tears_down_the_project() {
        let opts = ComposeOptions {
            services: vec!["web".into()],
            ..Default::default()
        };
        let args = build(ComposeAction::Down, &target(), &opts);
        assert!(
            !args.contains(&"web".to_string()),
            "a partial `down` would be a confusing no-op"
        );
    }

    #[test]
    fn profiles_come_before_the_subcommand() {
        let opts = ComposeOptions {
            profiles: vec!["debug".into()],
            ..Default::default()
        };
        let args = build(ComposeAction::Up, &target(), &opts);
        let profile = args.iter().position(|a| a == "--profile").unwrap();
        let up = args.iter().position(|a| a == "up").unwrap();
        assert!(profile < up, "compose rejects global flags after the verb");
    }

    #[test]
    fn build_and_force_recreate_reach_up() {
        let opts = ComposeOptions {
            build: true,
            force_recreate: true,
            ..Default::default()
        };
        let args = build(ComposeAction::Up, &target(), &opts);
        assert!(args.contains(&"--build".to_string()));
        assert!(args.contains(&"--force-recreate".to_string()));
    }

    #[test]
    fn pull_and_build_are_their_own_verbs() {
        assert!(build(ComposeAction::Pull, &target(), &ComposeOptions::default())
            .contains(&"pull".to_string()));
        assert!(build(ComposeAction::Build, &target(), &ComposeOptions::default())
            .contains(&"build".to_string()));
    }

    #[test]
    fn config_validates_the_file_set() {
        let args = config(&target());
        assert_eq!(args.last().unwrap(), "config");
        assert!(args.contains(&"-f".to_string()));
    }

    #[test]
    fn an_env_file_is_passed_through() {
        let target = ComposeTarget {
            files: vec!["c.yaml".into()],
            project: None,
            env_file: Some("/srv/.env".into()),
        };
        let args = build(ComposeAction::Up, &target, &ComposeOptions::default());
        let i = args.iter().position(|a| a == "--env-file").unwrap();
        assert_eq!(args[i + 1], "/srv/.env");
    }
}
