//! Locating and running the Compose binary.
//!
//! Hopper bundles a standalone Compose v2 so stacks work with no user-installed
//! `docker` CLI — that matters the moment someone actually uninstalls Docker
//! Desktop and its CLI goes with it.

use crate::client::Client;
use crate::error::{DockerError, Result};
use model::{ComposeProgress, StreamKind};
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

/// How Compose will be invoked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Compose {
    /// The standalone binary Hopper ships.
    Bundled(PathBuf),
    /// A `docker-compose` on PATH.
    Standalone(PathBuf),
    /// The `docker compose` plugin.
    Plugin,
}

impl Compose {
    /// The program and its leading arguments.
    pub fn program(&self) -> (String, Vec<String>) {
        match self {
            Compose::Bundled(p) | Compose::Standalone(p) => {
                (p.to_string_lossy().to_string(), vec![])
            }
            Compose::Plugin => ("docker".to_string(), vec!["compose".to_string()]),
        }
    }
}

/// Where the bundled binary sits inside the app bundle, relative to the
/// executable. Sidecars live in `Contents/MacOS/sidecars/`.
fn bundled_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let candidate = dir.join("sidecars").join("compose");
    candidate.is_file().then_some(candidate)
}

fn on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
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
        .env("DOCKER_HOST", client.endpoint().docker_host_value())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    if let Some(dir) = workdir.filter(|d| !d.trim().is_empty()) {
        cmd.current_dir(dir);
    }

    let mut child = cmd.spawn().map_err(|e| {
        DockerError::transport(format!("Could not start {program}: {e}"))
    })?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(StreamKind, String)>();

    if let Some(out) = stdout {
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(out).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if tx.send((StreamKind::Stdout, line)).is_err() {
                    break;
                }
            }
        });
    }
    if let Some(err) = stderr {
        // Compose writes its progress to stderr, so this is the interesting
        // stream, not an error channel.
        tokio::spawn(async move {
            let mut lines = BufReader::new(err).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if tx.send((StreamKind::Stderr, line)).is_err() {
                    break;
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
    fn a_standalone_binary_is_invoked_directly() {
        let (program, args) = Compose::Standalone(PathBuf::from("/usr/local/bin/docker-compose")).program();
        assert_eq!(program, "/usr/local/bin/docker-compose");
        assert!(args.is_empty());
    }

    #[test]
    fn the_bundled_binary_is_invoked_directly_too() {
        let (program, args) = Compose::Bundled(PathBuf::from("/Apps/Hopper.app/sidecars/compose")).program();
        assert!(program.ends_with("compose"));
        assert!(args.is_empty());
    }
}
