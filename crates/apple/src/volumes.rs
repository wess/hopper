//! Volumes, through the `container` CLI.

use docker::Result;
use model::{InspectResult, PruneReport, Volume};

use crate::cli::Cli;
use crate::wire::VolumeResource;

pub async fn list(cli: &Cli) -> Result<Vec<Volume>> {
    let raw: Vec<VolumeResource> = cli.json(&["volume", "list"]).await?;
    Ok(raw.into_iter().map(VolumeResource::into_model).collect())
}

pub async fn inspect(cli: &Cli, name: &str) -> Result<InspectResult> {
    let raw = cli.run(&["volume", "inspect", name]).await?;
    let value: serde_json::Value = crate::cli::decode(&raw)?;
    Ok(match value {
        serde_json::Value::Array(mut items) if !items.is_empty() => items.remove(0),
        other => other,
    })
}

pub async fn create(cli: &Cli, name: &str) -> Result<Volume> {
    cli.ok(&["volume", "create", name]).await?;
    // Read it back so the caller gets the real record rather than a guess at
    // the defaults Apple chose.
    let all = list(cli).await?;
    Ok(all
        .into_iter()
        .find(|v| v.name == name)
        .unwrap_or_else(|| Volume {
            name: name.to_string(),
            driver: "local".into(),
            scope: "local".into(),
            size: -1,
            ..Default::default()
        }))
}

pub async fn remove(cli: &Cli, name: &str) -> Result<()> {
    cli.ok(&["volume", "delete", name]).await
}

pub async fn prune(cli: &Cli) -> Result<PruneReport> {
    let before = list(cli).await.map(|v| v.len()).unwrap_or(0);
    cli.ok(&["volume", "prune"]).await?;
    let after = list(cli).await.map(|v| v.len()).unwrap_or(0);
    Ok(PruneReport {
        kind: "volumes".into(),
        removed: before.saturating_sub(after) as i64,
        reclaimed: 0,
    })
}
