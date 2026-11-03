use anyhow::{ensure, Context};
use engine::machines::{
  native::{
    config,
    sessions::{remote, Sessions},
    snapshot::Policy,
    State,
  },
  Actor, Machines,
};
use std::{
  fs,
  os::unix::fs::{FileExt, PermissionsExt},
};

pub async fn verify(
  sessions: &Sessions,
  manager: &Machines,
  paths: &config::Paths,
  id: &str,
  snapshot: &str,
) -> anyhow::Result<()> {
  let before = Policy::person(manager.clone(), id)?.inventory()?.len();
  let path = paths
    .root
    .join("snapshots")
    .join(snapshot.replace('-', ""))
    .join("ram.hrm");
  let reader = fs::File::open(&path)?;
  let mut original = [0];
  reader.read_exact_at(&mut original, 0)?;
  fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
  let writer = fs::OpenOptions::new().write(true).open(&path)?;
  writer.write_all_at(&[original[0] ^ 1], 0)?;
  fs::set_permissions(&path, fs::Permissions::from_mode(0o400))?;
  let outcome = remote::restoration::restore(
    manager,
    id,
    manager.machine(id, Actor::Agent)?.agent_generation,
    snapshot,
  )
  .await;
  writer.write_all_at(&original, 0)?;
  writer.sync_all()?;
  let error = outcome
    .err()
    .context("Corrupted checkpoint unexpectedly restored")?;
  ensure!(
    format!("{error:#}").contains("checksum"),
    "Unexpected integrity error: {error:#}"
  );
  ensure!(
    matches!(
      sessions.state(id, Actor::Person).await?,
      Some(State::Running)
    ),
    "Integrity rejection disrupted the running source"
  );
  ensure!(
    Policy::person(manager.clone(), id)?.inventory()?.len() == before
      && !paths.root.join("restore.hrr").exists(),
    "Integrity rejection captured prior state or selected a generation"
  );
  println!("Signed agent restore rejects a damaged component body with its footer unchanged before prior capture or source stop, preserving the running VM and saved checkpoint inventory");
  Ok(())
}
