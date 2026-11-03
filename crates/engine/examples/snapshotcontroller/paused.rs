use anyhow::{ensure, Context};
use engine::machines::{
  native::{config, sessions::Sessions, snapshot::Policy, State},
  Actor, Machines,
};
use machine::{
  runtime::hardware,
  snapshot::{checkpoint, package::Package},
};
use model::native::{Boot, Command};
use std::{fs::File, time::Duration};

pub fn counter(manager: &Machines, boot: &Boot, snapshot: &str) -> anyhow::Result<[u8; 4]> {
  let paths = config::paths(&manager.root, &boot.machine_id)?;
  let policy = Policy::person(manager.clone(), &boot.machine_id)?;
  let package = Package::inspect(
    &File::open(paths.root.join("snapshots"))?,
    model::native::machine_identity(&boot.machine_id).context("Missing counter VM identity")?,
    model::native::machine_identity(snapshot).context("Missing counter snapshot identity")?,
    || policy.check(),
  )?;
  let (_, devices, _) = hardware::checkpoint(
    boot.clone(),
    &package,
    package.binding().configuration,
    || policy.check(),
  )?;
  let saved = checkpoint::load(&package, &devices, || policy.check())?;
  let mut bytes = [0; 4];
  saved.primary.memory.ram.read(1 << 20, &mut bytes)?;
  Ok(bytes)
}

pub async fn unchanged(
  sessions: &Sessions,
  manager: &Machines,
  boot: &Boot,
  expected: &str,
) -> anyhow::Result<()> {
  let id = &boot.machine_id;
  let watch = sessions.watch(id).await?;
  ensure!(
    matches!(*watch.borrow(), State::Paused),
    "Recovered viewer did not remain paused"
  );
  let capture = model::new_uuid();
  sessions
    .snapshot_named(id, Actor::Person, &capture, "Paused execution check")
    .await?;
  ensure!(
    counter(manager, boot, expected)? == counter(manager, boot, &capture)?,
    "Paused restoration executed guest RAM-counter firmware"
  );
  ensure!(
    matches!(*watch.borrow(), State::Paused),
    "Paused verification released guest CPUs"
  );
  Ok(())
}

pub async fn verify(
  sessions: &Sessions,
  manager: &Machines,
  boot: &Boot,
  snapshot: &str,
) -> anyhow::Result<()> {
  let id = &boot.machine_id;
  sessions
    .request(id, Actor::Person, Command::Pause {})
    .await?;
  let watch = sessions.watch(id).await?;
  sessions
    .restore_snapshot(id, Actor::Agent, snapshot)
    .await?;
  unchanged(sessions, manager, boot, snapshot).await?;
  if boot.timeout_ms.is_none() {
    // activation must retire the admission deadline while keeping the CPU hold.
    tokio::time::sleep(Duration::from_secs(31)).await;
    unchanged(sessions, manager, boot, snapshot).await?;
  }
  sessions
    .request(id, Actor::Person, Command::Resume {})
    .await?;
  ensure!(
    matches!(*watch.borrow(), State::Running),
    "Explicit resume did not release paused restore"
  );
  tokio::time::sleep(Duration::from_millis(100)).await;
  let running = model::new_uuid();
  sessions
    .snapshot_named(id, Actor::Person, &running, "Resumed execution check")
    .await?;
  ensure!(
    counter(manager, boot, snapshot)? != counter(manager, boot, &running)?,
    "Resumed RAM-counter firmware did not execute"
  );
  println!("Signed paused restore activates without guest execution, keeps its viewer paused beyond admission expiry and changes the captured guest RAM counter only after explicit resume");
  Ok(())
}
