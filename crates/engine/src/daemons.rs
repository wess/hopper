//! The Docker-compatible engines Hopper knows by name.
//!
//! `existing` attaches to whatever `DOCKER_HOST` or the default socket points
//! at, which is one engine and gives the user no say. Plenty of machines have
//! several installed at once — Docker Desktop and Podman and Colima are happy
//! side by side — so this names each one and where it listens, and the picker
//! offers them separately.
//!
//! Pure over an environment and an `exists` predicate: which sockets a machine
//! really has is the caller's business, so the path rules are unit-tested
//! without touching the filesystem.

/// A daemon Hopper can name, and the sockets it is known to listen on.
///
/// `paths` is in preference order. A rootless install is listed before the
/// system one: someone running rootless meant to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Daemon {
    /// Stable, persisted in settings and accepted by `HOPPER_ENGINE`.
    pub id: &'static str,
    pub label: &'static str,
    pub paths: Vec<String>,
    /// Windows daemons listen on named pipes rather than filesystem sockets.
    pub npipe: bool,
}

impl Daemon {
    /// The first socket that is really there.
    pub fn socket(&self, exists: &dyn Fn(&str) -> bool) -> Option<String> {
        self.paths.iter().find(|p| exists(p)).cloned()
    }
}

/// What the path rules depend on. Separated so tests can vary it.
#[derive(Clone, Debug, Default)]
pub struct Env {
    pub home: Option<String>,
    pub xdg_runtime_dir: Option<String>,
    pub temp_dir: Option<String>,
}

impl Env {
    pub fn current() -> Self {
        Self {
            home: std::env::var("HOME").ok(),
            xdg_runtime_dir: std::env::var("XDG_RUNTIME_DIR").ok(),
            // GUI-launched processes are not guaranteed to inherit TMPDIR.
            // `temp_dir` still resolves to the platform's real temporary
            // directory on macOS, where Podman publishes its API socket.
            temp_dir: std::env::var("TMPDIR")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .or_else(|| Some(std::env::temp_dir().to_string_lossy().into_owned())),
        }
    }
}

/// Every named daemon worth offering on this platform.
///
/// The ids are stable across platforms even though the paths are not: someone
/// who pinned `podman` on Linux and syncs their settings to a Mac should get
/// Podman there too, not a dangling preference.
pub fn known(os: &str, env: &Env) -> Vec<Daemon> {
    if os == "windows" {
        return vec![
            Daemon {
                id: "docker",
                label: "Docker Desktop",
                paths: vec![r"\\.\pipe\docker_engine".into()],
                npipe: true,
            },
            Daemon {
                id: "podman",
                label: "Podman",
                paths: vec![r"\\.\pipe\podman-machine-default".into()],
                npipe: true,
            },
        ];
    }
    if os != "macos" && os != "linux" {
        return Vec::new();
    }
    let home = env.home.clone().unwrap_or_default();
    let xdg = env.xdg_runtime_dir.clone();

    let mut docker: Vec<String> = Vec::new();
    let mut podman: Vec<String> = Vec::new();

    if os == "macos" {
        // Docker Desktop moved to a per-user socket; the system one is a
        // symlink it also maintains, so it is the fallback rather than first.
        docker.push(format!("{home}/.docker/run/docker.sock"));
        // Podman on a Mac is a Linux VM. Current Podman exposes the API at a
        // per-user temporary socket returned by `podman machine inspect`.
        // Keep the older persistent path as a compatibility fallback because
        // existing machines can survive a Podman upgrade.
        if let Some(temp) = &env.temp_dir {
            podman.push(format!("{temp}/podman/podman-machine-default-api.sock"));
        }
        podman.push(format!(
            "{home}/.local/share/containers/podman/machine/podman.sock"
        ));
    } else {
        // Rootless first on Linux: it is a deliberate choice, and the system
        // socket is often present but not the one the user means.
        if let Some(runtime) = &xdg {
            docker.push(format!("{runtime}/docker.sock"));
            podman.push(format!("{runtime}/podman/podman.sock"));
        }
        podman.push("/run/podman/podman.sock".into());
    }
    docker.push("/var/run/docker.sock".into());

    let docker = Daemon {
        id: "docker",
        label: if os == "macos" {
            "Docker Desktop"
        } else {
            "Docker"
        },
        paths: docker,
        npipe: false,
    };
    let podman = Daemon {
        id: "podman",
        label: "Podman",
        paths: podman,
        npipe: false,
    };

    // Podman leads on Linux, which is the order `providers::linux` has always
    // auto-detected in: a rootless install is a deliberate choice, and both
    // sockets are often present at once. Keeping it means upgrading does not
    // silently move a Podman user onto Docker. macOS has no such history, and
    // Apple's runtime leads there anyway.
    let mut out = if os == "linux" {
        vec![podman, docker]
    } else {
        vec![docker, podman]
    };

    out.push(Daemon {
        id: "colima",
        label: "Colima",
        paths: vec![format!("{home}/.colima/default/docker.sock")],
        npipe: false,
    });
    // Rancher Desktop is a desktop app, and on Linux it reuses the docker
    // socket rather than one of its own.
    if os == "macos" {
        out.push(Daemon {
            id: "rancher",
            label: "Rancher Desktop",
            paths: vec![format!("{home}/.rd/docker.sock")],
            npipe: false,
        });
    }
    out
}

/// The named daemon ids for a platform, in the order selection should try them.
pub fn ids(os: &str) -> Vec<&'static str> {
    known(os, &Env::default())
        .into_iter()
        .map(|d| d.id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> Env {
        Env {
            home: Some("/Users/dev".into()),
            xdg_runtime_dir: Some("/run/user/1000".into()),
            temp_dir: Some("/var/folders/xx/T".into()),
        }
    }

    fn find<'a>(list: &'a [Daemon], id: &str) -> &'a Daemon {
        list.iter().find(|d| d.id == id).expect("daemon is known")
    }

    #[test]
    fn a_mac_is_offered_the_engines_that_actually_run_there() {
        let ids = ids("macos");
        assert!(ids.contains(&"docker"));
        assert!(ids.contains(&"podman"));
        assert!(ids.contains(&"colima"));
        assert!(ids.contains(&"rancher"));
    }

    #[test]
    fn linux_is_not_offered_the_desktop_only_engines() {
        // Rancher Desktop is a desktop app; offering it on a server would be a
        // row that can never become available.
        assert!(!ids("linux").contains(&"rancher"));
    }

    #[test]
    fn docker_desktops_per_user_socket_is_preferred_over_the_system_one() {
        let list = known("macos", &env());
        let d = find(&list, "docker");
        assert_eq!(
            d.socket(&|p| p == "/Users/dev/.docker/run/docker.sock" || p == "/var/run/docker.sock"),
            Some("/Users/dev/.docker/run/docker.sock".into())
        );
    }

    #[test]
    fn rootless_beats_the_system_socket_on_linux() {
        // Running rootless is a deliberate choice, and both sockets are often
        // present at once.
        let list = known("linux", &env());
        assert_eq!(
            find(&list, "docker").socket(&|_| true),
            Some("/run/user/1000/docker.sock".into())
        );
        assert_eq!(
            find(&list, "podman").socket(&|_| true),
            Some("/run/user/1000/podman/podman.sock".into())
        );
    }

    #[test]
    fn the_system_socket_still_answers_when_there_is_no_rootless_one() {
        let list = known("linux", &env());
        assert_eq!(
            find(&list, "docker").socket(&|p| p == "/var/run/docker.sock"),
            Some("/var/run/docker.sock".into())
        );
        assert_eq!(
            find(&list, "podman").socket(&|p| p == "/run/podman/podman.sock"),
            Some("/run/podman/podman.sock".into())
        );
    }

    #[test]
    fn a_daemon_with_no_socket_on_this_machine_reports_none() {
        // Which is what makes the picker say "not installed" rather than
        // offering a button that cannot work.
        let list = known("macos", &env());
        assert_eq!(find(&list, "colima").socket(&|_| false), None);
    }

    #[test]
    fn macos_podman_prefers_the_current_machine_api_socket() {
        let list = known("macos", &env());
        let podman = find(&list, "podman");
        assert_eq!(
            podman.socket(&|p| p == "/var/folders/xx/T/podman/podman-machine-default-api.sock"),
            Some("/var/folders/xx/T/podman/podman-machine-default-api.sock".into())
        );
    }

    #[test]
    fn windows_names_docker_and_podman_named_pipes() {
        let list = known("windows", &Env::default());
        assert_eq!(ids("windows"), vec!["docker", "podman"]);
        assert!(find(&list, "docker").npipe);
        assert_eq!(find(&list, "docker").paths[0], r"\\.\pipe\docker_engine");
        assert!(find(&list, "podman").npipe);
    }

    #[test]
    fn the_ids_are_the_same_on_every_platform_even_though_the_paths_are_not() {
        // A synced setting that says `podman` has to mean Podman on both.
        for id in ["docker", "podman", "colima"] {
            assert!(ids("macos").contains(&id));
            assert!(ids("linux").contains(&id));
        }
    }

    #[test]
    fn a_mac_without_a_home_directory_still_produces_usable_paths() {
        // `HOME` is always set in practice, but an empty one must not make the
        // system socket unreachable.
        let list = known("macos", &Env::default());
        assert_eq!(
            find(&list, "docker").socket(&|p| p == "/var/run/docker.sock"),
            Some("/var/run/docker.sock".into())
        );
    }
}
