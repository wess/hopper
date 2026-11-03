use crate::machines::native::{self, restore::Owner, Client};
use anyhow::{ensure, Context};
use model::native::{Command, Result as Reply, StopReason};

pub(super) async fn finish(client: &Client, owner: &Owner) -> anyhow::Result<()> {
  // cleanup may stop only the worker admitted by this request, including after revocation.
  let stopped = native::request(client, Command::Stop {}).await;
  native::finished(client)
    .await
    .context("Held recovery worker cleanup is unverified")?;
  ensure!(
    matches!(
      stopped?,
      Reply::Stopped {
        reason: StopReason::Requested
      }
    ),
    "Held recovery worker did not acknowledge requested stop"
  );
  owner.check()
}
