use super::{Owner, Phase};
use crate::machines::native::{self as native, startup, Client};
use anyhow::{bail, ensure, Context};
use model::native::{Boot, Command, Restore, Result as Reply};
use std::{
  path::Path,
  sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
  },
};
use tokio::sync::oneshot;

struct Cancel(Arc<AtomicBool>);
impl Drop for Cancel {
  fn drop(&mut self) {
    self.0.store(true, Ordering::Release);
  }
}

async fn cleanup(client: &Client) -> anyhow::Result<()> {
  let _ = native::request(client, Command::Stop {}).await;
  native::finished(client).await
}

pub async fn launch_worker(
  helper: &Path,
  boot: Boot,
  restore: Restore,
  owner: Arc<Owner>,
) -> anyhow::Result<Client> {
  launch(helper, boot, restore, owner, false).await
}

pub async fn launch_generation(
  helper: &Path,
  boot: Boot,
  restore: Restore,
  owner: Arc<Owner>,
) -> anyhow::Result<Client> {
  launch(helper, boot, restore, owner, true).await
}

async fn launch(
  helper: &Path,
  boot: Boot,
  restore: Restore,
  owner: Arc<Owner>,
  system: bool,
) -> anyhow::Result<Client> {
  owner.boot(&boot)?;
  let selected = owner.selected()?.context("No host restore selection")?;
  ensure!(
    selected.phase
      == if system {
        Phase::Active
      } else {
        Phase::Attempted
      }
      && Some(selected.machine) == model::native::machine_identity(&boot.machine_id)
      && Some(selected.snapshot) == model::native::machine_identity(&restore.snapshot_id)
      && Some(selected.generation) == model::native::machine_identity(&restore.generation_id)
      && owner
        .policy()
        .generation()
        .is_none_or(|tag| tag == restore.policy_generation),
    "Restored startup differs from original host selection/policy"
  );
  let cancelled = Arc::new(AtomicBool::new(false));
  let _cancel = Cancel(cancelled.clone());
  let helper = helper.to_owned();
  let (send, receive) = oneshot::channel();
  tokio::spawn(async move {
    let original = owner.policy();
    let authority = owner.clone();
    let context = boot.clone();
    let check: native::Check = Arc::new(move || {
      ensure!(
        !cancelled.load(Ordering::Acquire),
        "Restored startup cancelled"
      );
      original.check()?;
      authority.boot(&context)
    });
    let scope = startup::Scope {
      check: check.clone(),
      keep: owner.clone(),
    };
    let result = async {
      let kind = if system {
        startup::Kind::Generation(restore)
      } else {
        startup::Kind::Restore(restore)
      };
      let client = startup::launch(&helper, boot, kind, Some(scope)).await?;
      let validated = (|| {
        check()?;
        ensure!(
          client.configuration == selected.configuration,
          "Restored worker hardware differs from host selection"
        );
        Ok::<_, anyhow::Error>(())
      })();
      if let Err(error) = validated {
        cleanup(&client).await?;
        return Err(error);
      }
      Ok(client)
    }
    .await;
    if let Err(Ok(client)) = send.send(result) {
      // the caller left after admission; the monitor retains ownership through actual process exit.
      let _ = cleanup(&client).await;
    }
  });
  receive
    .await
    .context("Restored startup owner ended without a result")?
}

pub async fn resume_worker(
  client: &Client,
  restore: Restore,
  owner: Arc<Owner>,
) -> anyhow::Result<()> {
  resume(client, restore, owner, false, false).await
}

pub async fn pause_worker(
  client: &Client,
  restore: Restore,
  owner: Arc<Owner>,
) -> anyhow::Result<()> {
  resume(client, restore, owner, false, true).await
}

pub async fn resume_generation(
  client: &Client,
  restore: Restore,
  owner: Arc<Owner>,
) -> anyhow::Result<()> {
  resume(client, restore, owner, true, false).await
}

async fn resume(
  client: &Client,
  restore: Restore,
  owner: Arc<Owner>,
  system: bool,
  paused: bool,
) -> anyhow::Result<()> {
  let cancelled = Arc::new(AtomicBool::new(false));
  let _cancel = Cancel(cancelled.clone());
  let client = client.clone();
  let (send, receive) = oneshot::channel();
  tokio::spawn(async move {
    let result = async {
      let selected = owner.selected()?.context("No host restore selection")?;
      ensure!(
        selected.phase
          == if system {
            Phase::Active
          } else {
            Phase::Attempted
          }
          && client.configuration == selected.configuration
          && Some(selected.snapshot) == model::native::machine_identity(&restore.snapshot_id)
          && Some(selected.generation) == model::native::machine_identity(&restore.generation_id)
          && owner
            .policy()
            .generation()
            .is_none_or(|tag| tag == restore.policy_generation),
        "Restore resume differs from original selection/policy"
      );
      let original = owner.policy();
      let authority = owner.clone();
      let check: native::Check = Arc::new(move || {
        ensure!(
          !cancelled.load(Ordering::Acquire),
          "Restored resume cancelled"
        );
        original.check()?;
        authority.check_namespace()
      });
      check()?;
      let _commit = if system {
        owner.commit()?
      } else {
        owner.activate(selected, || check())?
      };
      check()?;
      let command = if paused {
        Command::RestorePause { restore }
      } else {
        Command::RestoreResume { restore }
      };
      let packet = native::transact(&client, command, Some(check)).await?;
      if !matches!(
        (&packet.response.result, paused),
        (Reply::Running {}, false) | (Reply::Paused {}, true)
      ) {
        bail!("Restored worker refused explicit activation");
      }
      owner.check()
    }
    .await;
    if result.is_err() {
      let cleaned = cleanup(&client).await;
      let _ = send.send(result.and(cleaned));
    } else if send.send(result).is_err() {
      let _ = cleanup(&client).await;
    }
  });
  receive
    .await
    .context("Restored resume owner ended without a result")?
}
