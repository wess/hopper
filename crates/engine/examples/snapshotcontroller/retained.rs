use anyhow::{ensure, Context};
use engine::machines::{
  native::{
    config,
    restore::{self, Owner, Preparation},
    sessions::Sessions,
  },
  Actor, Machines,
};
use model::native::Boot;
use std::{
  fs,
  os::unix::fs::{FileExt, PermissionsExt},
  path::Path,
};

pub async fn verify(
  sessions: &Sessions,
  manager: &Machines,
  helper: &Path,
  boot: Boot,
  owner: Owner,
  snapshot: &str,
) -> anyhow::Result<()> {
  let id = boot.machine_id.clone();
  let paths = config::paths(&manager.root, &id)?;
  let expected = owner
    .selected()?
    .context("Missing retained preparation predecessor")?;
  let prior = uuid::Uuid::from_bytes(expected.snapshot).to_string();
  ensure!(
    prior != snapshot,
    "Diagnostic requires distinct source/prior checkpoints"
  );
  let path = paths
    .root
    .join("snapshots")
    .join(snapshot.replace('-', ""))
    .join("clock.hrt");
  let file = fs::File::open(&path)?;
  let mut original = [0];
  file.read_exact_at(&mut original, 0)?;
  fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
  let writer = fs::OpenOptions::new().write(true).open(&path)?;
  writer.write_all_at(&[original[0] ^ 1], 0)?;
  fs::set_permissions(&path, fs::Permissions::from_mode(0o400))?;
  let outcome =
    restore::prepare_retained(owner, boot.clone(), snapshot, &prior, Some(expected)).await;
  let Preparation { owner, result } = outcome?;
  let failure = result
    .err()
    .context("Corrupted checkpoint unexpectedly prepared")?;
  ensure!(
    owner.selected()? == Some(expected),
    "Failed preparation changed durable selection"
  );
  for path in [
    manager.root.join("locks").join(&id),
    manager.root.join("locks").join(format!("{id}.runtime")),
    paths.root.join(".restore"),
  ] {
    let file = fs::OpenOptions::new().read(true).write(true).open(path)?;
    let error = store::lock::exclusive(file)
      .err()
      .context("Preparation failure released an original owner")?;
    ensure!(
      error.kind() == std::io::ErrorKind::WouldBlock,
      "Unexpected retained-lock failure: {error}"
    );
  }
  owner.check()?;
  let policy = owner.policy();
  let recovered = restore::recover_preparation(
    owner,
    failure,
    restore::Recovery {
      boot,
      target: snapshot.into(),
      prior: prior.clone(),
      predecessor: Some(expected),
      configuration: expected.configuration,
      check: std::sync::Arc::new(move || policy.check()),
    },
  )
  .await?;
  let prepared = recovered.prepared;
  ensure!(
    prepared.restore.snapshot_id == prior,
    "Continuation did not select the saved prior"
  );
  sessions
    .start_restored(&id, helper, prepared.boot, prepared.restore, prepared.owner)
    .await?;
  let mut damaged = [0];
  file.read_exact_at(&mut damaged, 0)?;
  ensure!(
    damaged != original,
    "Recovery repaired or admitted the corrupt target"
  );
  writer.write_all_at(&original, 0)?;
  writer.sync_all()?;
  sessions.stop(&id, Actor::Person).await?;
  println!("Signed preparation failure retains original ownership and recovers the valid prior through a fresh independent recovery backup while the failed target remains corrupt, without original mutable paths");
  Ok(())
}
