//! The Linux engine: whichever of Docker or Podman is actually installed.
//!
//! Both speak the Engine API, so once the socket is found the rest of Hopper
//! is unchanged — Podman's `podman.sock` is deliberately Docker-compatible.
//! What this provider adds is finding it, and saying which one answered, so
//! the UI can name the engine the user actually runs.
//!
//! Rootless installs put the socket under `XDG_RUNTIME_DIR`, which is where a
//! desktop Podman lives; the system paths are checked after.

use async_trait::async_trait;
use docker::client::Client;
use docker::Endpoint;
use model::{EngineState, EngineStatus};

use crate::provider::Provider;

/// Which daemon a socket belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flavour {
    Docker,
    Podman,
}

impl Flavour {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Docker => "Docker",
            Self::Podman => "Podman",
        }
    }
}

/// A socket we could talk to, and what is on the other end.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub flavour: Flavour,
    pub path: String,
}

/// Every socket worth trying, most specific first.
///
/// `DOCKER_HOST` wins outright when set — a user pointing at a remote or
/// non-standard daemon means it, and probing local paths would quietly ignore
/// them.
pub fn candidates(env: &Env) -> Vec<Found> {
    let mut out = Vec::new();

    if let Some(runtime) = env.xdg_runtime_dir.as_deref() {
        // Rootless Podman, the common desktop case.
        out.push(Found {
            flavour: Flavour::Podman,
            path: format!("{runtime}/podman/podman.sock"),
        });
        // Rootless Docker.
        out.push(Found {
            flavour: Flavour::Docker,
            path: format!("{runtime}/docker.sock"),
        });
    }
    out.push(Found {
        flavour: Flavour::Docker,
        path: "/var/run/docker.sock".into(),
    });
    out.push(Found {
        flavour: Flavour::Podman,
        path: "/run/podman/podman.sock".into(),
    });
    if let Some(home) = env.home.as_deref() {
        out.push(Found {
            flavour: Flavour::Podman,
            path: format!("{home}/.local/share/containers/podman/machine/podman.sock"),
        });
    }
    out
}

/// The environment the candidate list depends on, so it can be tested without
/// touching the real one.
#[derive(Clone, Debug, Default)]
pub struct Env {
    pub xdg_runtime_dir: Option<String>,
    pub home: Option<String>,
    pub docker_host: Option<String>,
}

impl Env {
    pub fn from_process() -> Self {
        Self {
            xdg_runtime_dir: std::env::var("XDG_RUNTIME_DIR").ok(),
            home: std::env::var("HOME").ok(),
            docker_host: std::env::var("DOCKER_HOST").ok(),
        }
    }
}

/// The first candidate whose socket exists on disk.
///
/// Existence only — whether the daemon answers is `status()`'s job, and a
/// socket that is present but dead still tells us which engine to name.
pub fn detect(env: &Env, exists: &dyn Fn(&str) -> bool) -> Option<Found> {
    candidates(env).into_iter().find(|c| exists(&c.path))
}

pub struct Linux {
    client: Client,
}

impl Linux {
    pub fn new(client: Client) -> Self {
        Self { client }
    }

    fn found(&self) -> Option<Found> {
        let env = Env::from_process();
        if env.docker_host.is_some() {
            return None;
        }
        detect(&env, &|p| std::path::Path::new(p).exists())
    }
}

#[async_trait]
impl Provider for Linux {
    fn id(&self) -> &'static str {
        "linux"
    }

    fn label(&self) -> &'static str {
        "Docker or Podman"
    }

    /// Only on Linux, and only when a socket is actually there. Otherwise the
    /// fallback provider takes over and reports what the user can do.
    async fn available(&self) -> bool {
        cfg!(target_os = "linux") && self.found().is_some()
    }

    async fn endpoint(&self) -> Option<Endpoint> {
        self.found().map(|f| Endpoint::Unix { path: f.path })
    }

    async fn status(&self) -> EngineStatus {
        let Some(found) = self.found() else {
            return EngineStatus::new(
                EngineState::NotInstalled,
                "linux",
                "No Docker or Podman socket was found.",
            )
            .detail(
                "Install Docker or Podman, or start its socket — for Podman that is \
                 `systemctl --user start podman.socket`."
                    .to_string(),
            );
        };

        let name = found.flavour.label();
        match self.client.ping().await {
            Ok(()) => EngineStatus::new(EngineState::Connected, "linux", format!("Connected to {name}."))
                .endpoint(found.path),
            Err(e) => {
                let mut status = crate::status_from(&e, "linux", false, &found.path);
                if status.state == EngineState::Stopped {
                    status.message = format!("{name} is installed but not running.");
                    if found.flavour == Flavour::Podman {
                        status.detail = Some(
                            "Start it with `systemctl --user start podman.socket`.".to_string(),
                        );
                    }
                }
                status
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> Env {
        Env {
            xdg_runtime_dir: Some("/run/user/1000".into()),
            home: Some("/home/wess".into()),
            docker_host: None,
        }
    }

    #[test]
    fn rootless_podman_is_preferred_over_the_system_docker_socket() {
        // A desktop Linux user running rootless Podman should not be silently
        // attached to a stale root Docker socket.
        let found = detect(&env(), &|p| {
            p == "/run/user/1000/podman/podman.sock" || p == "/var/run/docker.sock"
        })
        .unwrap();
        assert_eq!(found.flavour, Flavour::Podman);
        assert_eq!(found.path, "/run/user/1000/podman/podman.sock");
    }

    #[test]
    fn docker_is_found_when_it_is_the_only_one_there() {
        let found = detect(&env(), &|p| p == "/var/run/docker.sock").unwrap();
        assert_eq!(found.flavour, Flavour::Docker);
    }

    #[test]
    fn system_podman_is_found_when_there_is_no_docker() {
        let found = detect(&env(), &|p| p == "/run/podman/podman.sock").unwrap();
        assert_eq!(found.flavour, Flavour::Podman);
    }

    #[test]
    fn a_machine_with_neither_reports_nothing_rather_than_guessing() {
        assert!(detect(&env(), &|_| false).is_none());
    }

    #[test]
    fn rootless_docker_beats_the_system_socket() {
        let found = detect(&env(), &|p| {
            p == "/run/user/1000/docker.sock" || p == "/var/run/docker.sock"
        })
        .unwrap();
        assert_eq!(found.path, "/run/user/1000/docker.sock");
    }

    #[test]
    fn without_a_runtime_dir_the_system_paths_still_work() {
        let bare = Env { xdg_runtime_dir: None, home: None, docker_host: None };
        let found = detect(&bare, &|p| p == "/var/run/docker.sock").unwrap();
        assert_eq!(found.flavour, Flavour::Docker);
        // And nothing panics or produces a `/podman.sock` from a missing var.
        assert!(!candidates(&bare).iter().any(|c| c.path.starts_with("/podman")));
    }

    #[tokio::test]
    async fn it_is_never_available_off_linux() {
        let p = Linux::new(Client::new(Endpoint::Unix { path: "/nope.sock".into() }));
        if !cfg!(target_os = "linux") {
            assert!(!p.available().await);
        }
        assert_eq!(p.id(), "linux");
        assert!(!p.managed(), "Hopper does not own another vendor's daemon");
    }
}
