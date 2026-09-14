//! Compose stack operations.

pub mod args;
pub mod files;
pub mod runner;

use crate::client::Client;
use crate::error::{DockerError, Result};
use model::{
    ComposeAction, ComposeConfigResult, ComposeOptions, ComposeProgress, ComposeTarget, StreamKind,
};

const MAX_CONFIG_OUTPUT_BYTES: usize = 16 * 1024 * 1024;

/// Run a lifecycle action over a stack, streaming Compose's output.
pub async fn run<F>(
    client: &Client,
    request_id: &str,
    action: ComposeAction,
    target: &ComposeTarget,
    opts: &ComposeOptions,
    on_line: F,
) -> Result<i32>
where
    F: FnMut(ComposeProgress) -> bool,
{
    let argv = args::build(action, target, opts);
    let workdir = files::working_dir(target);
    runner::run(client, request_id, &argv, workdir.as_deref(), on_line).await
}

/// Validate a compose file set with `docker compose config`.
pub async fn config(client: &Client, target: &ComposeTarget) -> Result<ComposeConfigResult> {
    let argv = args::config(target);
    let workdir = files::working_dir(target);

    let mut yaml = String::new();
    let mut errors = String::new();
    let mut oversized = false;
    let code = runner::run(client, "config", &argv, workdir.as_deref(), |p| {
        if p.done {
            return true;
        }
        if yaml
            .len()
            .saturating_add(errors.len())
            .saturating_add(p.line.len() + 1)
            > MAX_CONFIG_OUTPUT_BYTES
        {
            oversized = true;
            return false;
        }
        match p.stream {
            StreamKind::Stdout => {
                yaml.push_str(&p.line);
                yaml.push('\n');
            }
            StreamKind::Stderr => {
                errors.push_str(&p.line);
                errors.push('\n');
            }
        }
        true
    })
    .await?;

    if oversized {
        return Err(DockerError::decode(format!(
            "Compose config output exceeded {} MiB.",
            MAX_CONFIG_OUTPUT_BYTES / (1024 * 1024)
        )));
    }

    Ok(if code == 0 {
        ComposeConfigResult {
            ok: true,
            yaml: Some(yaml),
            error: None,
        }
    } else {
        ComposeConfigResult {
            ok: false,
            yaml: None,
            error: Some(if errors.trim().is_empty() {
                format!("Compose exited with status {code}.")
            } else {
                errors.trim().to_string()
            }),
        }
    })
}

/// Whether Compose is available at all, so the UI can hide what cannot work.
pub fn available() -> bool {
    runner::discover().is_some()
}
