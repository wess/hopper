use anyhow::{ensure, Context};
use engine::machines::{
  native::{
    restore::{self, Owner, Phase},
    sessions::Sessions,
    State,
  },
  Actor, Machines,
};
use model::native::Boot;
use std::{
  fs,
  os::unix::fs::{FileExt, PermissionsExt},
  path::Path,
};

pub async fn resume(manager: &Machines, id: &str, helper: &Path) -> anyhow::Result<()> {
  let sessions = Sessions::new(manager.clone());
  use engine::machines::native::{assets, config};
  use sha2::{Digest, Sha256};
  let machine = manager.machine(id, Actor::Person)?;
  let paths = config::paths(&manager.root, id)?;
  let (_, firmware) = assets::candidates(&std::env::current_exe()?)?;
  let digest = format!(
    "{:x}",
    Sha256::digest(uuid::Uuid::parse_str(id)?.as_bytes())
  );
  let boot = Boot {
    machine_id: id.into(),
    firmware: firmware.join("windows.fd").to_string_lossy().into(),
    variables: firmware.join("variables.fd").to_string_lossy().into(),
    store: paths.variables.to_string_lossy().into(),
    disk: Some(paths.disk.to_string_lossy().into()),
    disk_id: format!("hpr{}", &digest[..17]),
    memory: u64::from(machine.resources.memory_gib) << 30,
    cpus: machine.resources.cpus,
    timeout_ms: None,
    boot_media: None,
    installer: None,
  };
  sessions
    .start_configured(id, &assets::locate()?, config::Stage::System)
    .await?;
  let watch = sessions.watch(id).await?;
  sessions.stop(id, Actor::Person).await?;
  verify(&sessions, manager, helper, boot, &watch).await
}

pub async fn verify(
  sessions: &Sessions,
  manager: &Machines,
  helper: &Path,
  boot: Boot,
  watch: &tokio::sync::watch::Receiver<State>,
) -> anyhow::Result<()> {
  for paused in [false, true] {
    attempt(sessions, manager, helper, boot.clone(), watch, paused).await?;
  }
  Ok(())
}

async fn attempt(
  sessions: &Sessions,
  manager: &Machines,
  helper: &Path,
  boot: Boot,
  watch: &tokio::sync::watch::Receiver<State>,
  paused: bool,
) -> anyhow::Result<()> {
  let id = boot.machine_id.clone();
  let root = Path::new(&boot.store)
    .parent()
    .context("Missing diagnostic namespace")?
    .to_owned();
  let owner = Owner::person(manager.clone(), &id)?;
  let previous = owner
    .selected()?
    .context("Missing prior active generation")?;
  let source = uuid::Uuid::from_bytes(previous.snapshot).to_string();
  let prior = uuid::Uuid::from_bytes(previous.rollback).to_string();
  let prepared = restore::prepare_successor(owner, boot.clone(), &source, &prior, previous).await?;
  let failed = prepared
    .owner
    .selected()?
    .context("Missing failed candidate")?;
  let path = root
    .join("snapshots")
    .join(source.replace('-', ""))
    .join("clock.hrt");
  let file = fs::File::open(&path)?;
  let mut original = [0];
  file.read_exact_at(&mut original, 0)?;
  fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
  let writer = fs::OpenOptions::new().write(true).open(&path)?;
  writer.write_all_at(&[original[0] ^ 1], 0)?;
  fs::set_permissions(&path, fs::Permissions::from_mode(0o400))?;
  let outcome = if paused {
    sessions.start_recoverable_paused(helper, prepared).await
  } else {
    sessions.start_recoverable(helper, prepared).await
  };
  let error = outcome
    .err()
    .context("Diagnostic restore unexpectedly succeeded")?;
  ensure!(
    error
      .to_string()
      .contains("saved prior checkpoint was recovered"),
    "Automatic recovery failed: {error:#}"
  );
  ensure!(
    matches!(
      (&*watch.borrow(), paused),
      (State::Running, false) | (State::Paused, true)
    ),
    "Rollback did not preserve requested lifecycle"
  );
  let mut damaged = [0];
  file.read_exact_at(&mut damaged, 0)?;
  ensure!(
    damaged != original,
    "Admission recovery repaired the failed target"
  );
  if paused {
    super::paused::unchanged(sessions, manager, &boot, &prior).await?;
  }
  writer.write_all_at(&original, 0)?;
  writer.sync_all()?;
  sessions.stop(&id, Actor::Person).await?;
  let owner = Owner::person(manager.clone(), &id)?;
  let recovered = owner
    .selected()?
    .context("Rollback selection disappeared")?;
  ensure!(
    recovered.phase == Phase::Active
      && recovered.snapshot == failed.rollback
      && recovered.previous == Some(failed.generation)
      && recovered.generation != failed.generation,
    "Rollback lost saved prior identity or reused failed generation"
  );
  ensure!(
    recovered.rollback != failed.snapshot && recovered.rollback != recovered.snapshot,
    "Admission recovery reused the failed target as its rollback checkpoint"
  );
  let archive = root.join(format!(
    "restore{}.hrr",
    uuid::Uuid::from_bytes(failed.generation).simple()
  ));
  ensure!(
    fs::read(archive)? == failed.encode()?,
    "Rollback rewrote failed selection history"
  );
  println!("Signed real-worker admission rejects a late-corrupted checkpoint; automatic rollback waits for cleanup, activates the saved prior (paused={paused}) with an independent backup while the target remains corrupt, preserves failed history and reports the original failure without original mutable paths");
  Ok(())
}
