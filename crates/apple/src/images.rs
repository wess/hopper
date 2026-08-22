//! Images, through the `container` CLI.

use docker::Result;
use model::{Image, InspectResult, PruneReport};

use crate::cli::Cli;
use crate::wire::ImageResource;

pub async fn list(cli: &Cli) -> Result<Vec<Image>> {
    let raw: Vec<ImageResource> = cli.json(&["image", "list"]).await?;
    Ok(raw.into_iter().map(ImageResource::into_model).collect())
}

pub async fn inspect(cli: &Cli, id: &str) -> Result<InspectResult> {
    let raw = cli.run(&["image", "inspect", id]).await?;
    let value: serde_json::Value = crate::cli::decode(&raw)?;
    Ok(match value {
        serde_json::Value::Array(mut items) if !items.is_empty() => items.remove(0),
        other => other,
    })
}

pub async fn pull(cli: &Cli, reference: &str) -> Result<()> {
    // `--progress none` keeps the spinner's control codes out of the buffer we
    // capture; progress reaches the UI from the caller instead.
    cli.ok(&["image", "pull", "--progress", "none", reference])
        .await
}

pub async fn push(cli: &Cli, reference: &str) -> Result<()> {
    cli.ok(&["image", "push", "--progress", "none", reference])
        .await
}

pub async fn tag(cli: &Cli, source: &str, target: &str) -> Result<()> {
    cli.ok(&["image", "tag", source, target]).await
}

pub async fn remove(cli: &Cli, id: &str) -> Result<()> {
    cli.ok(&["image", "delete", id]).await
}

/// Save an image to a tar on the host. The import path uses this in reverse.
pub async fn save(cli: &Cli, reference: &str, dest: &std::path::Path) -> Result<()> {
    let dest = dest.to_string_lossy().into_owned();
    cli.ok(&["image", "save", "--output", &dest, reference]).await
}

/// Load an image from a tar produced by `docker save` or `container save`.
pub async fn load(cli: &Cli, src: &std::path::Path) -> Result<()> {
    let src = src.to_string_lossy().into_owned();
    cli.ok(&["image", "load", "--input", &src]).await
}

pub async fn prune(cli: &Cli, all: bool) -> Result<PruneReport> {
    let before = list(cli).await.map(|i| i.len()).unwrap_or(0);
    let mut args = vec!["image", "prune"];
    if all {
        args.push("--all");
    }
    cli.ok(&args).await?;
    let after = list(cli).await.map(|i| i.len()).unwrap_or(0);
    Ok(PruneReport {
        kind: "images".into(),
        removed: before.saturating_sub(after) as i64,
        reclaimed: 0,
    })
}
