//! Locating and running the Compose binary.
//!
//! Hopper bundles Compose for optional Docker-compatible engines. Apple's
//! runtime has no Docker socket, so its stacks use the host's own planner.

use crate::client::Client;
use crate::error::{DockerError, Result};
use futures::StreamExt;
use model::{ComposeProgress, StreamKind};
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;
use tokio_util::codec::{FramedRead, LinesCodec};

const MAX_COMPOSE_LINE_BYTES: usize = 1024 * 1024;

/// How Compose will be invoked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Compose {
    /// The standalone binary Hopper ships.
    Bundled(PathBuf),
    /// A `docker-compose` on PATH.
    Standalone(PathBuf),
    /// The `docker compose` plugin.
    Plugin,
    /// The `docker compose` plugin using Hopper's bundled Docker CLI.
    BundledPlugin(PathBuf),
}

impl Compose {
    /// The program and its leading arguments.
    pub fn program(&self) -> (String, Vec<String>) {
        match self {
            Compose::Bundled(p) | Compose::Standalone(p) => {
                (p.to_string_lossy().to_string(), vec![])
            }
            Compose::Plugin => ("docker".to_string(), vec!["compose".to_string()]),
            Compose::BundledPlugin(p) => {
                (p.to_string_lossy().to_string(), vec!["compose".to_string()])
            }
        }
    }
}

/// Where the bundled binary sits inside the app bundle, relative to the
/// executable. Sidecars live in `Contents/MacOS/sidecars/`.
fn bundled_candidate_from(exe: &std::path::Path) -> Option<PathBuf> {
    Some(
        exe.parent()?
            .join("sidecars")
            .join(executable_name("compose")),
    )
}

fn bundled_path_from(exe: &std::path::Path) -> Option<PathBuf> {
    let candidate = bundled_candidate_from(exe)?;
    candidate.is_file().then_some(candidate)
}

fn bundled_docker_candidate_from(exe: &std::path::Path) -> Option<PathBuf> {
    Some(
        exe.parent()?
            .join("sidecars")
            .join(executable_name("docker")),
    )
}

#[cfg(windows)]
fn executable_name(name: &str) -> String {
    format!("{name}.exe")
}

#[cfg(not(windows))]
fn executable_name(name: &str) -> String {
    name.to_string()
}

fn bundled_docker_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let candidate = bundled_docker_candidate_from(&exe)?;
    candidate.is_file().then_some(candidate)
}

fn bundled_path() -> Option<PathBuf> {
    bundled_path_from(&std::env::current_exe().ok()?)
}

fn on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let names = if cfg!(windows) {
        vec![format!("{name}.exe"), name.to_string()]
    } else {
        vec![name.to_string()]
    };
    std::env::split_paths(&path)
        .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
        .find(|p| p.is_file())
}

/// Pick the Compose implementation to use, preferring the one we ship.
pub fn discover() -> Option<Compose> {
    if let Some(p) = bundled_path() {
        return Some(Compose::Bundled(p));
    }
    if let Some(p) = on_path("docker-compose") {
        return Some(Compose::Standalone(p));
    }
    if let Some(p) = bundled_docker_path() {
        return Some(Compose::BundledPlugin(p));
    }
    on_path("docker").map(|_| Compose::Plugin)
}

/// Run a Compose command, streaming each output line to `on_line`.
///
/// `DOCKER_HOST` is set from the active endpoint so Compose targets the same
/// engine the rest of the app is talking to, rather than whatever the user's
/// shell happens to point at.
pub async fn run<F>(
    client: &Client,
    request_id: &str,
    args: &[String],
    workdir: Option<&str>,
    mut on_line: F,
) -> Result<i32>
where
    F: FnMut(ComposeProgress) -> bool,
{
    let Some(compose) = discover() else {
        return Err(DockerError::transport(
            "No Compose binary was found. Hopper ships one; this build appears to be missing it."
                .to_string(),
        ));
    };
    let (program, mut argv) = compose.program();
    argv.extend_from_slice(args);

    let mut cmd = Command::new(&program);
    cmd.args(&argv)
        .kill_on_drop(true)
        .env_remove("DOCKER_TLS_VERIFY")
        .env("DOCKER_HOST", client.endpoint().docker_host_value())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    if matches!(client.endpoint(), crate::Endpoint::Tcp { tls: true, .. }) {
        cmd.env("DOCKER_TLS_VERIFY", "1");
    }
    if let Some(dir) = workdir.filter(|d| !d.trim().is_empty()) {
        cmd.current_dir(dir);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| DockerError::transport(format!("Could not start {program}: {e}")))?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    // Compose can be very chatty (especially while pulling images). Keep a
    // bounded queue so a slow UI consumer applies backpressure to the child
    // process instead of letting output grow without limit in Hopper.
    let (tx, mut rx) = tokio::sync::mpsc::channel::<(StreamKind, String)>(256);

    if let Some(out) = stdout {
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut lines =
                FramedRead::new(out, LinesCodec::new_with_max_length(MAX_COMPOSE_LINE_BYTES));
            while let Some(result) = lines.next().await {
                match result {
                    Ok(line) => {
                        if tx.send((StreamKind::Stdout, line)).await.is_err() {
                            break;
                        }
                    }
                    Err(error) => {
                        if tx
                            .send((
                                StreamKind::Stdout,
                                format!("[Compose output line omitted: {error}]"),
                            ))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
        });
    }
    if let Some(err) = stderr {
        // Compose writes its progress to stderr, so this is the interesting
        // stream, not an error channel.
        tokio::spawn(async move {
            let mut lines =
                FramedRead::new(err, LinesCodec::new_with_max_length(MAX_COMPOSE_LINE_BYTES));
            while let Some(result) = lines.next().await {
                match result {
                    Ok(line) => {
                        if tx.send((StreamKind::Stderr, line)).await.is_err() {
                            break;
                        }
                    }
                    Err(error) => {
                        if tx
                            .send((
                                StreamKind::Stderr,
                                format!("[Compose output line omitted: {error}]"),
                            ))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
        });
    } else {
        drop(tx);
    }

    while let Some((stream, line)) = rx.recv().await {
        let keep = on_line(ComposeProgress {
            request_id: request_id.to_string(),
            line,
            stream,
            done: false,
            error: None,
        });
        if !keep {
            let _ = child.kill().await;
            break;
        }
    }

    let status = child
        .wait()
        .await
        .map_err(|e| DockerError::transport(format!("{program} did not exit cleanly: {e}")))?;
    let code = status.code().unwrap_or(-1);

    on_line(ComposeProgress {
        request_id: request_id.to_string(),
        line: String::new(),
        stream: StreamKind::Stdout,
        done: true,
        error: (code != 0).then(|| format!("Compose exited with status {code}.")),
    });
    Ok(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plugin_form_puts_compose_after_docker() {
        let (program, args) = Compose::Plugin.program();
        assert_eq!(program, "docker");
        assert_eq!(args, vec!["compose"]);
    }

    #[test]
    fn the_bundled_docker_cli_is_invoked_as_a_plugin() {
        let (program, args) =
            Compose::BundledPlugin(PathBuf::from("/Apps/Hopper.app/sidecars/docker")).program();
        assert_eq!(program, "/Apps/Hopper.app/sidecars/docker");
        assert_eq!(args, vec!["compose"]);
    }

    #[test]
    fn a_standalone_binary_is_invoked_directly() {
        let (program, args) =
            Compose::Standalone(PathBuf::from("/usr/local/bin/docker-compose")).program();
        assert_eq!(program, "/usr/local/bin/docker-compose");
        assert!(args.is_empty());
    }

    #[test]
    fn the_bundled_binary_is_invoked_directly_too() {
        let (program, args) = Compose::Bundled(PathBuf::from(format!(
            "/Apps/Hopper.app/sidecars/{}",
            executable_name("compose")
        )))
        .program();
        assert!(program.ends_with(&executable_name("compose")));
        assert!(args.is_empty());
    }

    #[test]
    fn bundled_sidecars_are_next_to_the_app_executable() {
        let exe = PathBuf::from("/Applications/Hopper.app/Contents/MacOS/hopper");
        assert_eq!(
            bundled_candidate_from(&exe),
            Some(PathBuf::from(format!(
                "/Applications/Hopper.app/Contents/MacOS/sidecars/{}",
                executable_name("compose")
            ))),
        );
        assert_eq!(
            bundled_docker_candidate_from(&exe),
            Some(PathBuf::from(format!(
                "/Applications/Hopper.app/Contents/MacOS/sidecars/{}",
                executable_name("docker")
            ))),
        );
    }
}
