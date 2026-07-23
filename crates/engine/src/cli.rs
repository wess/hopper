//! Host `docker` CLI compatibility.
//!
//! Two separate problems, both required before someone can actually uninstall
//! Docker Desktop:
//!
//! 1. **Pointing the CLI at Hopper.** A Docker context named `hopper` does
//!    this for anything that respects contexts.
//! 2. **There being a CLI at all.** Docker Desktop's uninstaller takes the
//!    `docker` binary with it, so Hopper ships its own and can link it onto
//!    PATH.

use docker::Endpoint;
use model::{DockerCliSetupResult, DockerCliStatus};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;

pub const CONTEXT: &str = "hopper";
/// Where a user-local binary can go without touching system directories.
pub const SHIM_DIR: &str = "/usr/local/bin";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandResult {
    pub ok: bool,
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl CommandResult {
    /// The most useful line to show a user.
    pub fn detail(&self) -> String {
        let text = if !self.stderr.trim().is_empty() {
            &self.stderr
        } else if !self.stdout.trim().is_empty() {
            &self.stdout
        } else {
            return format!("docker exited with status {}.", self.code);
        };
        text.trim().to_string()
    }
}

async fn run(program: &str, args: &[&str]) -> CommandResult {
    match Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .output()
        .await
    {
        Ok(out) => CommandResult {
            ok: out.status.success(),
            code: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).to_string(),
            stderr: String::from_utf8_lossy(&out.stderr).to_string(),
        },
        Err(e) => CommandResult {
            ok: false,
            // 127 is the shell's "command not found", which is what this is.
            code: 127,
            stdout: String::new(),
            stderr: e.to_string(),
        },
    }
}

/// The commands that create or update the context and select it.
pub fn context_commands(host: &str, exists: bool) -> Vec<Vec<String>> {
    let mut first: Vec<String> = if exists {
        vec!["context".into(), "update".into(), CONTEXT.into()]
    } else {
        vec![
            "context".into(),
            "create".into(),
            CONTEXT.into(),
            "--description".into(),
            "Hopper managed Docker engine".into(),
        ]
    };
    first.push("--docker".into());
    first.push(format!("host={host}"));

    vec![
        first,
        vec!["context".into(), "use".into(), CONTEXT.into()],
    ]
}

/// The bundled CLI shipped alongside the app, if present.
pub fn bundled_cli() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let candidate = exe.parent()?.join("sidecars").join("docker");
    candidate.is_file().then_some(candidate)
}

/// Whether the `docker` on PATH is the one we installed.
pub fn shim_is_ours(shim: &Path) -> bool {
    std::fs::read_link(shim)
        .ok()
        .and_then(|target| {
            target
                .to_string_lossy()
                .contains("Hopper.app")
                .then_some(true)
        })
        .unwrap_or(false)
}

/// Report whether the CLI exists and where it points.
pub async fn status(endpoint: &Endpoint) -> DockerCliStatus {
    let host = endpoint.docker_host_value();
    let version = run("docker", &["--version"]).await;
    if !version.ok {
        return DockerCliStatus {
            available: false,
            configured: false,
            host,
            context: None,
            detail: if bundled_cli().is_some() {
                "The Docker CLI is not on your PATH. Hopper ships one and can install it."
                    .into()
            } else {
                "The Docker CLI was not found on your PATH.".into()
            },
            bundled: bundled_cli().is_some(),
        };
    }

    let current = run("docker", &["context", "show"]).await;
    let context = current.ok.then(|| current.stdout.trim().to_string());
    let configured = context.as_deref() == Some(CONTEXT);
    DockerCliStatus {
        available: true,
        configured,
        host,
        detail: if configured {
            "The Docker CLI is using Hopper.".into()
        } else {
            "The Docker CLI is not using Hopper.".into()
        },
        context,
        bundled: shim_is_ours(&PathBuf::from(SHIM_DIR).join("docker")),
    }
}

/// Create or update the `hopper` context and select it.
pub async fn setup(endpoint: &Endpoint) -> DockerCliSetupResult {
    let version = run("docker", &["--version"]).await;
    if !version.ok {
        let s = status(endpoint).await;
        return DockerCliSetupResult {
            ok: false,
            detail: s.detail.clone(),
            status: s,
        };
    }

    let host = endpoint.docker_host_value();
    let exists = run("docker", &["context", "inspect", CONTEXT]).await.ok;
    for argv in context_commands(&host, exists) {
        let args: Vec<&str> = argv.iter().map(String::as_str).collect();
        let result = run("docker", &args).await;
        if !result.ok {
            let s = status(endpoint).await;
            return DockerCliSetupResult {
                ok: false,
                detail: result.detail(),
                status: s,
            };
        }
    }

    DockerCliSetupResult {
        ok: true,
        detail: format!("The Docker CLI now targets {host}."),
        status: status(endpoint).await,
    }
}

/// Put Hopper's context back to whatever it was, by selecting `default`.
///
/// Configuring the CLI mutates the user's global Docker config, so there has
/// to be a way back that does not involve them learning the command.
pub async fn revert() -> CommandResult {
    run("docker", &["context", "use", "default"]).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creating_a_context_carries_a_description_but_updating_does_not() {
        let create = context_commands("unix:///x.sock", false);
        assert!(create[0].contains(&"create".to_string()));
        assert!(create[0].contains(&"--description".to_string()));

        let update = context_commands("unix:///x.sock", true);
        assert!(update[0].contains(&"update".to_string()));
        assert!(
            !update[0].contains(&"--description".to_string()),
            "docker context update rejects --description"
        );
    }

    #[test]
    fn the_host_is_passed_in_dockers_key_value_form() {
        let cmds = context_commands("unix:///Users/x/.hopper/run/docker.sock", false);
        assert!(cmds[0].contains(&"host=unix:///Users/x/.hopper/run/docker.sock".to_string()));
    }

    #[test]
    fn the_context_is_selected_after_being_created() {
        let cmds = context_commands("unix:///x.sock", false);
        assert_eq!(cmds.len(), 2);
        assert_eq!(cmds[1], vec!["context", "use", "hopper"]);
    }

    #[test]
    fn command_detail_prefers_stderr_then_stdout_then_the_status() {
        let stderr = CommandResult {
            ok: false,
            code: 1,
            stdout: "out".into(),
            stderr: "the real error".into(),
        };
        assert_eq!(stderr.detail(), "the real error");

        let stdout = CommandResult {
            ok: false,
            code: 1,
            stdout: "only stdout".into(),
            stderr: "  ".into(),
        };
        assert_eq!(stdout.detail(), "only stdout");

        let silent = CommandResult {
            ok: false,
            code: 3,
            stdout: String::new(),
            stderr: String::new(),
        };
        assert!(silent.detail().contains("status 3"));
    }
}
