//! Networks, through the `container` CLI.
//!
//! Creating a network needs macOS 26: the vmnet APIs that make more than one
//! network possible do not exist on 15, where every container shares `default`.

use docker::Result;
use model::{InspectResult, Network, NetworkCreateInput, PruneReport};

use crate::cli::Cli;
use crate::wire::NetworkResource;

pub async fn list(cli: &Cli) -> Result<Vec<Network>> {
    let raw: Vec<NetworkResource> = cli.json(&["network", "list"]).await?;
    Ok(raw.into_iter().map(NetworkResource::into_model).collect())
}

pub async fn inspect(cli: &Cli, id: &str) -> Result<InspectResult> {
    let raw = cli.run(&["network", "inspect", id]).await?;
    let value: serde_json::Value = crate::cli::decode(&raw)?;
    Ok(match value {
        serde_json::Value::Array(mut items) if !items.is_empty() => items.remove(0),
        other => other,
    })
}

pub async fn create(cli: &Cli, input: &NetworkCreateInput) -> Result<String> {
    let mut args: Vec<String> = vec!["network".into(), "create".into()];
    if let Some(subnet) = &input.subnet {
        args.push("--subnet".into());
        args.push(subnet.clone());
    }
    if input.internal {
        args.push("--internal".into());
    }
    args.push(input.name.clone());
    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    cli.ok(&borrowed).await?;
    Ok(input.name.clone())
}

pub async fn remove(cli: &Cli, id: &str) -> Result<()> {
    cli.ok(&["network", "delete", id]).await
}

pub async fn prune(cli: &Cli) -> Result<PruneReport> {
    let before = list(cli).await.map(|n| n.len()).unwrap_or(0);
    cli.ok(&["network", "prune"]).await?;
    let after = list(cli).await.map(|n| n.len()).unwrap_or(0);
    Ok(PruneReport {
        kind: "networks".into(),
        removed: before.saturating_sub(after) as i64,
        reclaimed: 0,
    })
}
