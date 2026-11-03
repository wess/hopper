use anyhow::{ensure, Context};
use engine::machines::{
  native::{config, restore::Owner, sessions::Sessions, State},
  Actor, Machines,
};
use model::native::{Boot, Command, Restore, Result as Reply};
use std::{path::Path, time::Duration};

pub async fn verify(
  sessions: &Sessions,
  manager: &Machines,
  paths: &config::Paths,
  helper: &Path,
  boot: Boot,
  owner: Owner,
  restore: Restore,
) -> anyhow::Result<()> {
  let id = boot.machine_id.clone();
  let bank = std::fs::read(paths.variables.join("bank"))?;
  sessions
    .start_restored(&id, helper, boot.clone(), restore.clone(), owner)
    .await?;
  let mut viewer = sessions.watch(&id).await?;
  ensure!(matches!(*viewer.borrow(), State::Running));
  ensure!(
    Owner::person(manager.clone(), &id).is_err(),
    "Restored runtime was not exclusive"
  );
  ensure!(matches!(
    sessions
      .request(&id, Actor::Person, Command::Status {})
      .await?,
    Reply::Status { paused: false, .. }
  ));
  sessions
    .request(&id, Actor::Person, Command::Pause {})
    .await?;
  tokio::time::timeout(Duration::from_secs(2), async {
    while !matches!(*viewer.borrow_and_update(), State::Paused) {
      viewer
        .changed()
        .await
        .context("Restored viewer state ended")?;
    }
    Ok::<_, anyhow::Error>(())
  })
  .await??;
  sessions
    .request(&id, Actor::Person, Command::Resume {})
    .await?;
  ensure!(matches!(
    sessions.state(&id, Actor::Person).await?,
    Some(State::Running)
  ));
  sessions.stop(&id, Actor::Person).await?;
  ensure!(matches!(*viewer.borrow(), State::Stopped(_)));
  let owner = Owner::person(manager.clone(), &id)?;
  ensure!(owner
    .selected()?
    .is_some_and(|r| r.phase == engine::machines::native::restore::Phase::Active));
  ensure!(
    config::initialize(manager, &id).is_err(),
    "Original boot bypassed pending restore"
  );
  ensure!(
    std::fs::read(paths.variables.join("bank"))? == bank,
    "Registry restored startup rewrote original EFI state"
  );
  super::system::verify(sessions, manager, paths, helper, boot, restore, owner).await?;
  println!("Signed registry restored session publishes viewer state, serves status/pause/resume under ordinary operation leases, retains exclusive runtime through stop/reap and preserves original EFI and pending recovery intent");
  Ok(())
}
