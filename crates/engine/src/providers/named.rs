//! A Docker-compatible engine Hopper knows by name.
//!
//! One per entry in [`crate::daemons`] — Docker Desktop, Podman, Colima,
//! Rancher Desktop. Each owns a socket rather than a lifecycle: Hopper attaches
//! to it, never starts it, which is what `managed = false` says.
//!
//! `DOCKER_HOST` disqualifies every one of them. A user pointing at a remote or
//! non-standard daemon means it, and quietly preferring a local socket that
//! happens to exist would ignore them — so selection falls through to
//! `existing`, which is the provider that honours it.

use async_trait::async_trait;
use docker::Endpoint;
use model::{EngineState, EngineStatus};
use std::time::Duration;

use crate::daemons::Daemon;
use crate::provider::Provider;

pub struct Named {
    daemon: Daemon,
    /// Set means an explicit Docker/Podman endpoint is pointing somewhere, so
    /// this provider stands aside. Held rather than read live so tests can set
    /// it.
    explicit_host: Option<String>,
}

impl Named {
    pub fn new(daemon: Daemon) -> Self {
        Self {
            daemon,
            explicit_host: ["DOCKER_HOST", "CONTAINER_HOST"]
                .into_iter()
                .find_map(|name| std::env::var(name).ok().filter(|v| !v.trim().is_empty())),
        }
    }

    /// The socket this daemon is listening on right now, if it is.
    fn socket(&self) -> Option<String> {
        if self.explicit_host.is_some() {
            return None;
        }
        if self.daemon.npipe {
            // Named pipes cannot be tested with Path::exists. Keep the
            // provider visible and let the bounded ping report its state.
            return self.daemon.paths.first().cloned();
        }
        self.daemon.socket(&|p| std::path::Path::new(p).exists())
    }
}

#[async_trait]
impl Provider for Named {
    fn id(&self) -> &'static str {
        self.daemon.id
    }

    fn label(&self) -> &'static str {
        self.daemon.label
    }

    async fn available(&self) -> bool {
        if self.daemon.npipe {
            // Unlike a Unix socket, a named pipe cannot be checked with
            // `Path::exists`. Opening it is the cheap, authoritative test that
            // a server is actually present; otherwise Settings would label
            // every conventional Docker/Podman pipe as installed.
            #[cfg(windows)]
            {
                use tokio::net::windows::named_pipe::ClientOptions;
                return self
                    .daemon
                    .paths
                    .first()
                    .is_some_and(|path| ClientOptions::new().open(path).is_ok());
            }
            #[cfg(not(windows))]
            return false;
        }
        self.socket().is_some()
    }

    async fn endpoint(&self) -> Option<Endpoint> {
        if self.explicit_host.is_some() {
            return None;
        }
        // Point at the daemon's canonical endpoint even while it is offline.
        // Otherwise selecting a stopped provider leaves the shared client
        // pointed at whichever engine happened to be active before it.
        self.socket()
            .or_else(|| self.daemon.paths.first().cloned())
            .map(|path| {
                if self.daemon.npipe {
                    Endpoint::Npipe { path }
                } else {
                    Endpoint::Unix { path }
                }
            })
    }

    async fn status(&self) -> EngineStatus {
        let label = self.daemon.label;

        if let Some(host) = &self.explicit_host {
            // Not a failure: the user pointed Hopper somewhere on purpose.
            return EngineStatus::new(
                EngineState::NotInstalled,
                self.daemon.id,
                format!("An explicit container endpoint is set to {host}, so Hopper is using that instead."),
            );
        }
        // Unlike a Unix socket, a named pipe cannot be checked with
        // `Path::exists`. `available()` opens the pipe on Windows, so use it
        // here too: an absent pipe is a missing/stopped engine, not a daemon
        // that accepted a connection and then failed to answer.
        if self.daemon.npipe && !self.available().await {
            return EngineStatus::new(
                EngineState::NotInstalled,
                self.daemon.id,
                format!("{label} is not running, or is not installed on this machine."),
            );
        }
        let Some(path) = self.socket() else {
            return EngineStatus::new(
                EngineState::NotInstalled,
                self.daemon.id,
                format!("{label} is not running, or is not installed on this machine."),
            );
        };

        // Probe a client dedicated to this provider. The registry asks every
        // row for status while the shared app client remains pointed at the
        // active engine; using that client here would make one row report
        // another row's health.
        let described = if self.daemon.npipe {
            format!("npipe:{path}")
        } else {
            format!("unix:{path}")
        };
        let endpoint = if self.daemon.npipe {
            Endpoint::Npipe { path }
        } else {
            Endpoint::Unix { path }
        };
        let client = docker::Client::new(endpoint);
        client.set_timeout(Duration::from_secs(3));
        match client.ping().await {
            Ok(()) => EngineStatus::new(EngineState::Connected, self.daemon.id, "Connected.")
                .endpoint(described),
            Err(e) => {
                let mut status = crate::status_from(&e, self.daemon.id, false, &described);
                // Nothing here is Hopper's to start, so say what the socket
                // being dead actually means rather than offering a dead button.
                if status.state == EngineState::Stopped {
                    status.state = EngineState::Unreachable;
                    status.message = format!(
                        "{label}'s {} is present but nothing answered on it.",
                        if self.daemon.npipe {
                            "named pipe"
                        } else {
                            "socket"
                        }
                    );
                    status.connected = false;
                }
                status
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemons::{known, Env};

    fn daemon(id: &str) -> Daemon {
        known(
            "macos",
            &Env {
                home: Some("/Users/dev".into()),
                xdg_runtime_dir: None,
                temp_dir: None,
            },
        )
        .into_iter()
        .find(|d| d.id == id)
        .expect("daemon is known")
    }

    fn provider(id: &str, explicit_host: Option<&str>) -> Named {
        Named {
            daemon: daemon(id),
            explicit_host: explicit_host.map(str::to_string),
        }
    }

    #[tokio::test]
    async fn a_daemon_keeps_its_id_and_name_for_the_picker() {
        let p = provider("colima", None);
        assert_eq!(p.id(), "colima");
        assert_eq!(p.label(), "Colima");
        assert!(!p.managed(), "Hopper does not own these lifecycles");
    }

    #[tokio::test]
    async fn docker_host_stands_every_named_daemon_down() {
        // Otherwise a local socket that happens to exist would quietly win
        // over the remote the user asked for.
        let p = provider("docker", Some("tcp://build-box:2375"));
        assert!(!p.available().await);
        assert!(p.endpoint().await.is_none());
    }

    #[tokio::test]
    async fn a_docker_host_user_is_told_where_hopper_went_instead() {
        let s = provider("docker", Some("tcp://build-box:2375"))
            .status()
            .await;
        assert!(s.message.contains("build-box"), "{}", s.message);
        assert!(!s.connected);
    }

    #[tokio::test]
    async fn an_engine_that_is_not_installed_says_so_by_name() {
        // Colima's socket lives under a home directory this test does not have,
        // so this exercises the real "no socket" path.
        let s = provider("colima", None).status().await;
        assert_eq!(s.state, EngineState::NotInstalled);
        assert!(s.message.contains("Colima"), "{}", s.message);
        assert!(!s.message.is_empty());
    }

    #[tokio::test]
    async fn an_offline_provider_still_exposes_its_own_endpoint() {
        let p = provider("colima", None);
        assert_eq!(
            p.endpoint()
                .await
                .and_then(|endpoint| endpoint.path().map(str::to_string)),
            Some("/Users/dev/.colima/default/docker.sock".into())
        );
    }

    #[tokio::test]
    #[cfg(not(windows))]
    async fn an_unavailable_named_pipe_is_not_reported_as_unreachable() {
        let p = Named {
            daemon: Daemon {
                id: "docker",
                label: "Docker Desktop",
                paths: vec![r"\\.\pipe\docker_engine".into()],
                npipe: true,
            },
            explicit_host: None,
        };
        let status = p.status().await;
        assert_eq!(status.state, EngineState::NotInstalled);
        assert!(!status.connected);
    }
}
