use anyhow::{ensure, Context};
use engine::machines::{
  native::{self, config::Paths, restore::Owner},
  Machines,
};
use model::native::{Boot, Command, Restore, Result as Reply};
use std::{path::Path, time::Duration};

pub async fn verify(
  manager: &Machines,
  paths: &Paths,
  helper: &Path,
  boot: Boot,
  owner: Owner,
  restore: Restore,
) -> anyhow::Result<Owner> {
  let id = boot.machine_id.clone();
  let owner = std::sync::Arc::new(owner);
  let original_bank = std::fs::read(paths.variables.join("bank"))?;
  let worker = native::launch_restored(helper, boot, restore.clone(), owner.clone()).await?;
  ensure!(matches!(native::state(&worker), native::State::Paused));
  ensure!(native::request(&worker, Command::Resume {}).await.is_err());
  ensure!(
    native::request(
      &worker,
      Command::SnapshotStart {
        snapshot_id: model::new_uuid(),
        policy_generation: restore.policy_generation,
      }
    )
    .await
    .is_err(),
    "Checkpoint admission allowed capture before activation"
  );
  let mut stale = restore.clone();
  stale.policy_generation += 1;
  ensure!(
    native::request(&worker, Command::RestoreResume { restore: stale })
      .await
      .is_err()
  );
  ensure!(matches!(
    native::request(&worker, Command::Status {}).await?,
    Reply::Status { paused: true, .. }
  ));
  let pending = native::request(
    &worker,
    Command::RestoreResume {
      restore: restore.clone(),
    },
  )
  .await
  .err()
  .context("Restored worker executed before durable host activation")?;
  ensure!(
    pending.to_string().contains("not durably active"),
    "Preactivation resume failed for another reason: {pending:#}"
  );
  ensure!(matches!(
    native::request(&worker, Command::Status {}).await?,
    Reply::Status { paused: true, .. }
  ));
  native::resume_restored(&worker, restore.clone(), owner.clone()).await?;
  ensure!(matches!(
    native::request(&worker, Command::Status {}).await?,
    Reply::Status { paused: false, .. }
  ));
  ensure!(native::request(&worker, Command::RestoreResume { restore })
    .await
    .is_err());
  let retained = std::sync::Arc::downgrade(&owner);
  drop(owner);
  ensure!(retained.upgrade().is_some(), "Worker lost restore owner");
  ensure!(native::restore::Owner::person(manager.clone(), &id).is_err());
  native::request(&worker, Command::Stop {}).await?;
  native::finished(&worker).await?;
  drop(worker);
  ensure!(
    std::fs::read(paths.variables.join("bank"))? == original_bank,
    "Restored worker rewrote original EFI bank"
  );
  println!("Signed restored worker starts held, rejects ordinary/stale/replayed/preactivation resume, executes only after original explicit authorization, retires and preserves original EFI state");
  tokio::time::timeout(Duration::from_secs(2), async {
    while retained.upgrade().is_some() {
      tokio::time::sleep(Duration::from_millis(5)).await;
    }
  })
  .await
  .context("Restored worker retained ownership after confirmed reaping")?;
  println!("Signed restored worker retains exclusive host ownership after caller owner drop and releases it only after process reaping");
  let owner = native::restore::Owner::person(manager.clone(), &id)?;
  Ok(owner)
}
