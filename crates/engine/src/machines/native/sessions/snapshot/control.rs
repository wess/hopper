use crate::machines::native::{self as native, Check, Client};
use anyhow::{ensure, Context};
use model::native::{
  snapshot::{Receipt, Status},
  Command, Result as Reply,
};
use std::time::Duration;

async fn send(client: &Client, command: Command, check: &Check) -> anyhow::Result<Reply> {
  check()?;
  let request = native::transact(client, command, Some(check.clone()));
  tokio::pin!(request);
  loop {
    tokio::select! {
      reply = &mut request => return Ok(reply?.response.result),
      _ = tokio::time::sleep(Duration::from_millis(100)) => check()?,
    }
  }
}

pub(in crate::machines::native::sessions) async fn capture(
  client: &Client,
  snapshot: &str,
  tag: u64,
  check: &Check,
) -> anyhow::Result<Receipt> {
  let reply = send(
    client,
    Command::SnapshotStart {
      snapshot_id: snapshot.into(),
      policy_generation: tag,
    },
    check,
  )
  .await
  .context("Snapshot staging start failed")?;
  ensure!(
    matches!(reply, Reply::Accepted {}),
    "Snapshot worker did not accept capture"
  );
  loop {
    let Reply::Snapshot { status, .. } = send(
      client,
      Command::SnapshotPoll {
        snapshot_id: snapshot.into(),
        policy_generation: tag,
      },
      check,
    )
    .await
    .context("Snapshot staging poll failed")?
    else {
      anyhow::bail!("Snapshot worker returned no status");
    };
    match status {
      Status::Ready { receipt } => {
        check()?;
        return Ok(receipt);
      }
      Status::Failed {} => anyhow::bail!("Snapshot staging failed; private data retained"),
      Status::Capturing {} | Status::Staging {} => {}
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
  }
}

pub(in crate::machines::native::sessions) async fn cleanup(
  client: &Client,
  snapshot: &str,
  tag: u64,
) -> anyhow::Result<()> {
  // cleanup is confined to the original worker/job, even after user policy is revoked.
  let reply = native::request(
    client,
    Command::SnapshotCancel {
      snapshot_id: snapshot.into(),
      policy_generation: tag,
    },
  )
  .await
  .context("Snapshot worker cleanup failed")?;
  ensure!(
    matches!(reply, Reply::Accepted {}),
    "Snapshot cleanup was not acknowledged"
  );
  Ok(())
}
