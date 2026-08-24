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
use docker::client::Client;
use docker::Endpoint;
use model::{EngineState, EngineStatus};

use crate::daemons::Daemon;
use crate::provider::Provider;

pub struct Named {
    daemon: Daemon,
    client: Client,
    /// Set means `DOCKER_HOST` is pointing somewhere, so this provider stands
    /// aside. Held rather than read live so tests can set it.
    docker_host: Option<String>,
}

impl Named {
    pub fn new(daemon: Daemon, client: Client) -> Self {
        Self {
            daemon,
            client,
            docker_host: std::env::var("DOCKER_HOST").ok().filter(|v| !v.trim().is_empty()),
        }
    }

    /// The socket this daemon is listening on right now, if it is.
    fn socket(&self) -> Option<String> {
        if self.docker_host.is_some() {
            return None;
        }
        self.daemon
            .socket(&|p| std::path::Path::new(p).exists())
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
        self.socket().is_some()
    }

    async fn endpoint(&self) -> Option<Endpoint> {
        self.socket().map(|path| Endpoint::Unix { path })
    }

    async fn status(&self) -> EngineStatus {
        let label = self.daemon.label;

        if let Some(host) = &self.docker_host {
            // Not a failure: the user pointed Hopper somewhere on purpose.
            return EngineStatus::new(
                EngineState::NotInstalled,
                self.daemon.id,
                format!("DOCKER_HOST is set to {host}, so Hopper is using that instead."),
            );
        }
        let Some(path) = self.socket() else {
            return EngineStatus::new(
                EngineState::NotInstalled,
                self.daemon.id,
                format!("{label} is not running, or is not installed on this machine."),
            );
        };

        // The socket is there, so ask whether anything is behind it. `select`
        // repoints the client before calling this, so the ping is this daemon's.
        let described = format!("unix:{path}");
        match self.client.ping().await {
            Ok(()) => EngineStatus::new(EngineState::Connected, self.daemon.id, "Connected.")
                .endpoint(described),
            Err(e) => {
                let mut status = crate::status_from(&e, self.daemon.id, false, &described);
                // Nothing here is Hopper's to start, so say what the socket
                // being dead actually means rather than offering a dead button.
                if status.state == EngineState::Stopped {
                    status.state = EngineState::Unreachable;
                    status.message =
                        format!("{label}'s socket is there but nothing answered on it.");
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
            },
        )
        .into_iter()
        .find(|d| d.id == id)
        .expect("daemon is known")
    }

    fn provider(id: &str, docker_host: Option<&str>) -> Named {
        Named {
            daemon: daemon(id),
            client: Client::from_env(),
            docker_host: docker_host.map(str::to_string),
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
        let s = provider("docker", Some("tcp://build-box:2375")).status().await;
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
}
