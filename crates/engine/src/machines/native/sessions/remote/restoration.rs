use super::{wire, Operation, Reply};
use crate::machines::{
  native::{
    sessions::{Registry, Restored, Sessions},
    snapshot::Policy,
  },
  Machines,
};
use anyhow::{ensure, Context};
use std::{sync::Weak, time::Duration};
use tokio::{io::AsyncReadExt, net::UnixStream};

/// Only saved identities cross this boundary; authorization predates upstream waits.
pub async fn restore(
  manager: &Machines,
  id: &str,
  generation: u64,
  snapshot: &str,
) -> anyhow::Result<Restored> {
  let policy = Policy::agent(manager.clone(), id, generation)?;
  model::native::machine_identity(snapshot).context("Invalid restore snapshot identity")?;
  let result = tokio::time::timeout(Duration::from_secs(915), async {
    let mut stream = super::connect(
      manager,
      id,
      Operation::Restore {
        id: snapshot.into(),
      },
      generation,
    )
    .await?;
    let reply: Reply = wire::read(&mut stream).await.context(
      "Restore service ended without a receipt; inspect VM recovery state before retrying",
    )?;
    match reply {
      Reply::Restored {
        snapshot_id,
        rollback_id,
        generation_id,
      } => {
        ensure!(
          snapshot_id == snapshot
            && rollback_id != snapshot_id
            && [&snapshot_id, &rollback_id, &generation_id]
              .into_iter()
              .all(|id| model::native::machine_identity(id).is_some()),
          "Restore service returned invalid checkpoint/generation identities"
        );
        Ok(Restored {
          snapshot_id,
          rollback_id,
          generation_id,
        })
      }
      Reply::Error { message } => {
        ensure!(
          message.len() <= 2048 && !message.contains('\0'),
          "Invalid restore service error"
        );
        anyhow::bail!("{message}");
      }
      _ => anyhow::bail!("Unexpected restore service reply"),
    }
  })
  .await
  .context("Restore service timed out; inspect VM recovery state before retrying")?;
  policy.check()?;
  result
}

pub(super) async fn serve(
  mut stream: UnixStream,
  registry: Weak<Registry>,
  id: String,
  generation: Option<u64>,
  snapshot: String,
) -> anyhow::Result<()> {
  let registry = registry
    .upgrade()
    .context("Hopper native VM registry is closed")?;
  let operation = async {
    let generation = generation.context("Restore requires the original agent generation")?;
    let policy = Policy::agent(registry.manager.clone(), &id, generation)?;
    let sessions = Sessions {
      inner: registry.clone(),
    };
    let restored = sessions
      .restore_snapshot_with_generation(&id, &snapshot, generation)
      .await?;
    policy.check()?;
    Ok::<_, anyhow::Error>((policy, restored))
  };
  let mut trailing = [0; 1];
  let result = tokio::select! {
    result = operation => result,
    _ = stream.read(&mut trailing) => anyhow::bail!("Restore agent disconnected or sent another request"),
  };
  let reply = match result {
    Ok((policy, restored)) => {
      policy.check()?;
      Reply::Restored {
        snapshot_id: restored.snapshot_id,
        rollback_id: restored.rollback_id,
        generation_id: restored.generation_id,
      }
    }
    Err(error) => Reply::Error {
      message: format!("{error:#}").chars().take(512).collect(),
    },
  };
  wire::write(&mut stream, &reply, &[]).await
}
