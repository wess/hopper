use anyhow::{ensure, Context};
use engine::machines::{
  native::{config, restore::Owner, sessions::Sessions, snapshot::Policy, State},
  Actor, Machines,
};
use model::native::{Boot, Command};
use std::{fs, os::unix::fs::FileExt, path::Path};

pub async fn verify(
  sessions: &Sessions,
  manager: &Machines,
  paths: &config::Paths,
  helper: &Path,
  boot: Boot,
  id: &str,
  snapshot: &str,
) -> anyhow::Result<()> {
  manager.set_agent_access(id, true)?;
  if std::env::var("HOPPER_RESTORE_PAUSED").as_deref() == Ok("1") {
    super::fallback::verify(sessions, manager, paths, &boot, snapshot, true).await?;
    super::paused::verify(sessions, manager, &boot, snapshot).await?;
    let watch = sessions.watch(id).await?;
    sessions.stop(id, Actor::Person).await?;
    fs::rename(&paths.disk, paths.disk.with_extension("retained"))?;
    fs::rename(&paths.variables, paths.root.join("originalvariables"))?;
    return super::rollback::verify(sessions, manager, helper, boot, &watch).await;
  }
  super::integrity::verify(sessions, manager, paths, id, snapshot).await?;
  super::admission::verify(sessions, manager, paths, id, snapshot).await?;
  super::fallback::verify(sessions, manager, paths, &boot, snapshot, false).await?;
  super::fallback::verify(sessions, manager, paths, &boot, snapshot, true).await?;
  super::paused::verify(sessions, manager, &boot, snapshot).await?;
  let baseline = Policy::person(manager.clone(), id)?.inventory()?.len();
  let original = fs::read(paths.variables.join("bank"))?;
  let watch = sessions.watch(id).await?;
  let restored = if let Some(executable) = std::env::var_os("HOPPER_RESTORE_MCP") {
    super::agent::restore(
      manager
        .root
        .parent()
        .context("Missing MCP diagnostic profile")?,
      id,
      snapshot,
      &executable,
    )
    .await?
  } else {
    engine::machines::native::sessions::remote::restoration::restore(
      manager,
      id,
      manager.machine(id, Actor::Agent)?.agent_generation,
      snapshot,
    )
    .await?
  };
  ensure!(
    restored.snapshot_id == snapshot && restored.rollback_id != snapshot,
    "Automatic restore lost source/prior identity"
  );
  ensure!(
    matches!(*watch.borrow(), State::Running),
    "Original viewer watch did not follow restore"
  );
  let entries = Policy::person(manager.clone(), id)?.inventory()?;
  ensure!(
    entries.len() == baseline + 1
      && entries
        .iter()
        .any(|entry| entry.id == restored.rollback_id
          && entry.name.as_deref() == Some("Before restore")),
    "Automatic prior snapshot was not published"
  );
  let first = engine::machines::native::restore::read(
    &fs::File::open(&paths.root)?,
    model::native::machine_identity(id).unwrap(),
  )?
  .context("First restored selection disappeared")?;
  let repeated = sessions
    .restore_snapshot(id, Actor::Agent, snapshot)
    .await?;
  ensure!(
    repeated.generation_id != restored.generation_id
      && repeated.rollback_id != restored.rollback_id,
    "Repeated restore reused generation or prior checkpoint identity"
  );
  let archive = paths.root.join(format!(
    "restore{}.hrr",
    restored.generation_id.replace('-', "")
  ));
  ensure!(
    fs::read(archive)? == first.encode()?,
    "Repeated restore did not preserve predecessor"
  );
  ensure!(
    matches!(*watch.borrow(), State::Running),
    "Repeated restore lost original viewer watch"
  );
  let entries = Policy::person(manager.clone(), id)?.inventory()?;
  ensure!(
    entries.len() == baseline + 2
      && entries
        .iter()
        .any(|entry| entry.id == repeated.rollback_id
          && entry.name.as_deref() == Some("Before restore")),
    "Repeated restore did not capture fresh prior state"
  );
  let restored = repeated;
  sessions
    .request(id, Actor::Person, Command::Pause {})
    .await?;
  sessions
    .request(id, Actor::Person, Command::Resume {})
    .await?;
  sessions.stop(id, Actor::Person).await?;
  let owner = Owner::person(manager.clone(), id)?;
  let selected = owner
    .selected()?
    .context("Automatic restore selection disappeared")?;
  ensure!(
    selected.phase == engine::machines::native::restore::Phase::Active
      && Some(selected.snapshot) == model::native::machine_identity(snapshot)
      && Some(selected.rollback) == model::native::machine_identity(&restored.rollback_id)
      && Some(selected.generation) == model::native::machine_identity(&restored.generation_id),
    "Automatic restore selection was not bound to its prior state"
  );
  ensure!(
    selected.previous == Some(first.generation),
    "Repeated restore lost predecessor linkage"
  );
  println!("Signed repeated running-source restore captures a fresh named prior checkpoint, creates an independent generation, retains predecessor history/resources and follows the original viewer watch");
  ensure!(
    fs::read(paths.variables.join("bank"))? == original,
    "Automatic restore rewrote original firmware"
  );
  let mut bytes = [0; 20];
  fs::File::open(&paths.disk)?.read_exact_at(&mut bytes, 0)?;
  ensure!(
    &bytes == b"original source disk",
    "Automatic restore changed original disk"
  );
  println!("Signed automatic agent restore pauses and captures a fresh named recoverable prior state, stops/reaps its source, transfers operation ownership through production preparation and activation, keeps its viewer watch, binds rollback identity and preserves original disk/EFI");
  let retained_disk = paths.disk.with_extension("retained");
  let retained_variables = paths.root.join("originalvariables");
  fs::rename(&paths.disk, &retained_disk)?;
  fs::rename(&paths.variables, &retained_variables)?;
  drop(owner);
  let tag = manager.machine(id, Actor::Agent)?.agent_generation;
  let stopped = if std::env::var("HOPPER_RESTORE_CONFIGURED").as_deref() == Ok("1") {
    sessions
      .restore_snapshot_with_generation(id, snapshot, tag)
      .await?
  } else {
    let owner = Owner::agent(manager.clone(), id, tag)?;
    sessions
      .restore_stopped(helper, boot.clone(), snapshot, owner)
      .await?
  };
  ensure!(
    stopped.generation_id != restored.generation_id && stopped.rollback_id != restored.rollback_id,
    "Stopped restore reused generation/prior identity"
  );
  ensure!(
    matches!(*watch.borrow(), State::Running),
    "Stopped restore lost original viewer watch"
  );
  let entries = Policy::person(manager.clone(), id)?.inventory()?;
  ensure!(
    entries.iter().any(
      |entry| entry.id == stopped.rollback_id && entry.name.as_deref() == Some("Before restore")
    ),
    "Stopped restore lost its captured prior state"
  );
  sessions.stop(id, Actor::Person).await?;
  let owner = Owner::person(manager.clone(), id)?;
  let next = owner
    .selected()?
    .context("Stopped restore selection disappeared")?;
  ensure!(
    next.previous == Some(selected.generation),
    "Stopped restore lost predecessor linkage"
  );
  let selected = next;
  let restored = stopped;
  println!("Signed owned stopped-generation restore captures a fresh named prior state before guest execution, reaps held hardware under continuous ownership, prepares a fresh successor without original mutable paths and preserves viewer continuity");
  let prior = engine::machines::native::restore::prepare_successor(
    owner,
    boot.clone(),
    &restored.rollback_id,
    snapshot,
    selected,
  )
  .await?;
  ensure!(
    prior.restore.generation_id != restored.generation_id
      && prior.restore.snapshot_id == restored.rollback_id,
    "Prior-state recovery reused the executed generation"
  );
  let recovered_generation = prior.restore.generation_id.clone();
  sessions
    .start_restored(id, helper, prior.boot, prior.restore, prior.owner)
    .await?;
  ensure!(
    matches!(*watch.borrow(), State::Running),
    "Viewer watch lost successor recovery"
  );
  sessions.stop(id, Actor::Person).await?;
  let owner = Owner::person(manager.clone(), id)?;
  let recovered = owner
    .selected()?
    .context("Successor selection disappeared")?;
  ensure!(
    recovered.phase == engine::machines::native::restore::Phase::Active
      && recovered.previous == Some(selected.generation)
      && Some(recovered.generation) == model::native::machine_identity(&recovered_generation),
    "Successor recovery lost its predecessor or active generation"
  );
  let archive = paths.root.join(format!(
    "restore{}.hrr",
    restored.generation_id.replace('-', "")
  ));
  ensure!(
    fs::read(archive)? == selected.encode()?,
    "Successor changed predecessor history"
  );
  ensure!(
    paths
      .root
      .join("generations")
      .join(restored.generation_id.replace('-', ""))
      .is_dir(),
    "Successor removed previous generation"
  );
  ensure!(
    !paths.disk.exists()
      && !paths.variables.exists()
      && fs::read(retained_variables.join("bank"))? == original,
    "Successor changed original firmware"
  );
  fs::File::open(&retained_disk)?.read_exact_at(&mut bytes, 0)?;
  ensure!(
    &bytes == b"original source disk",
    "Successor changed original disk"
  );
  println!("Signed owned successor preparation fully admits source/prior components, restores the saved prior checkpoint into a fresh generation without original disk/EFI paths, preserves predecessor history/resources and follows the original viewer watch");
  super::retained::verify(sessions, manager, helper, boot.clone(), owner, snapshot).await?;
  super::rollback::verify(sessions, manager, helper, boot, &watch).await?;
  Ok(())
}
