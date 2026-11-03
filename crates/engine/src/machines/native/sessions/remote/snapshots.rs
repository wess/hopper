use super::{wire, Operation, Reply};
use crate::machines::{
  native::{
    sessions::{Registry, Sessions},
    snapshot::Policy,
  },
  Machines,
};
use anyhow::{ensure, Context};
use std::{sync::Weak, time::Duration};
use tokio::{io::AsyncReadExt, net::UnixStream};

/// `generation` is captured by the caller before upstream waits; paths never cross this API.
pub async fn capture(
  manager: &Machines,
  id: &str,
  generation: u64,
  snapshot: &str,
  name: &str,
) -> anyhow::Result<()> {
  let policy = Policy::agent(manager.clone(), id, generation)?;
  model::native::machine_identity(snapshot).context("Invalid snapshot identity")?;
  let description = machine::snapshot::package::Description::new(name, 1)?;
  let reply = tokio::time::timeout(Duration::from_secs(315), async {
    let mut stream = super::connect(
      manager,
      id,
      Operation::Snapshot {
        id: snapshot.into(),
        name: description.name().into(),
      },
      generation,
    )
    .await?;
    let reply: Reply = wire::read(&mut stream).await?;
    match reply {
      Reply::Snapshot { id } => {
        ensure!(id == snapshot, "Snapshot service returned another identity")
      }
      Reply::Error { message } => {
        ensure!(
          message.len() <= 2048 && !message.contains('\0'),
          "Invalid snapshot service error"
        );
        anyhow::bail!("{message}");
      }
      _ => anyhow::bail!("Unexpected snapshot service reply"),
    }
    Ok::<_, anyhow::Error>(())
  })
  .await
  .context("Snapshot service timed out; check the saved catalog before retrying")?;
  policy.check()?;
  reply
}

pub(super) async fn serve(
  mut stream: UnixStream,
  registry: Weak<Registry>,
  id: String,
  generation: Option<u64>,
  snapshot: String,
  name: String,
) -> anyhow::Result<()> {
  let registry = registry
    .upgrade()
    .context("Hopper native VM registry is closed")?;
  let operation = async {
    let generation = generation.context("Snapshot requires the original agent generation")?;
    let policy = Policy::agent(registry.manager.clone(), &id, generation)?;
    let sessions = Sessions {
      inner: registry.clone(),
    };
    sessions
      .snapshot_named_with_generation(&id, &snapshot, &name, generation)
      .await?;
    policy.check()?;
    Ok::<_, anyhow::Error>(policy)
  };
  let mut trailing = [0; 1];
  let result = tokio::select! {
    result = operation => result,
    _ = stream.read(&mut trailing) => anyhow::bail!("Snapshot agent disconnected or sent another request"),
  };
  let reply = match result {
    Ok(policy) => {
      policy.check()?;
      Reply::Snapshot { id: snapshot }
    }
    Err(error) => Reply::Error {
      message: format!("{error:#}").chars().take(512).collect(),
    },
  };
  wire::write(&mut stream, &reply, &[]).await
}
