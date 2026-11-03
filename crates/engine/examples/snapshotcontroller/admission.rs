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
use machine::snapshot::{
  manifest::{Entry, Manifest},
  package::Package,
};
use sha2::{Digest, Sha256};
use std::{
  fs,
  os::unix::fs::{FileExt, PermissionsExt},
  path::Path,
};

fn replace(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
  fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
  fs::write(path, bytes)?;
  fs::set_permissions(path, fs::Permissions::from_mode(0o400))?;
  Ok(())
}

fn manifest(package: &Package, directory: &Path) -> anyhow::Result<Vec<u8>> {
  let entries = package
    .layout()
    .records()?
    .into_iter()
    .map(|record| {
      let file = fs::File::open(directory.join(record.name()))?;
      let length = file.metadata()?.len();
      ensure!(
        (36..=package.layout().limit(record)?).contains(&length),
        "Invalid diagnostic component size"
      );
      let mut checksum = [0; 32];
      file.read_exact_at(&mut checksum, length - 32)?;
      Ok(Entry {
        record,
        length,
        checksum,
      })
    })
    .collect::<anyhow::Result<Vec<_>>>()?;
  Ok(Manifest::new(package.binding(), package.layout(), entries)?.encode())
}

pub async fn verify(
  sessions: &Sessions,
  manager: &Machines,
  paths: &config::Paths,
  id: &str,
  snapshot: &str,
) -> anyhow::Result<()> {
  let parent = paths.root.join("snapshots");
  let directory = parent.join(snapshot.replace('-', ""));
  let package = Package::inspect(
    &fs::File::open(parent)?,
    model::native::machine_identity(id).unwrap(),
    model::native::machine_identity(snapshot).unwrap(),
    || Ok(()),
  )?;
  let clock = directory.join("clock.hrt");
  let receipt = directory.join("manifest.hsm");
  let original_clock = fs::read(&clock)?;
  let original_receipt = fs::read(&receipt)?;
  for image in [false, true] {
    let before = Policy::person(manager.clone(), id)?.inventory()?;
    let disk = directory.join("storage4/image");
    let mut original = [0];
    let writer = if image {
      fs::File::open(&disk)?.read_exact_at(&mut original, 8192)?;
      fs::set_permissions(&disk, fs::Permissions::from_mode(0o600))?;
      let writer = fs::OpenOptions::new().write(true).open(&disk)?;
      writer.write_all_at(&[original[0] ^ 1], 8192)?;
      fs::set_permissions(&disk, fs::Permissions::from_mode(0o400))?;
      Some(writer)
    } else {
      // a checksummed but invalid schema must pass the integrity precheck and fail decoding.
      let mut invalid = original_clock.clone();
      invalid[..4].copy_from_slice(b"BAD!");
      let end = invalid.len() - 32;
      let checksum = Sha256::digest(&invalid[..end]);
      invalid[end..].copy_from_slice(&checksum);
      replace(&clock, &invalid)?;
      replace(&receipt, &manifest(&package, &directory)?)?;
      None
    };
    let outcome = remote::restoration::restore(
      manager,
      id,
      manager.machine(id, Actor::Agent)?.agent_generation,
      snapshot,
    )
    .await;
    if let Some(writer) = writer {
      writer.write_all_at(&original, 8192)?;
      writer.sync_all()?;
    } else {
      replace(&clock, &original_clock)?;
      replace(&receipt, &original_receipt)?;
    }
    let error = outcome
      .err()
      .context("Invalid checkpoint unexpectedly restored")?;
    ensure!(
      format!("{error:#}").contains("Restore source validation failed before stop"),
      "Unexpected admission error: {error:#}"
    );
    ensure!(
      matches!(
        sessions.state(id, Actor::Person).await?,
        Some(State::Running)
      ),
      "Admission failure did not recover the source pause"
    );
    let after = Policy::person(manager.clone(), id)?.inventory()?;
    ensure!(
      after.len() == before.len() + 1
        && after
          .iter()
          .filter(|entry| !before.iter().any(|old| old.id == entry.id))
          .all(|entry| entry.name.as_deref() == Some("Before restore")),
      "Admission failure lost its fresh recoverable prior checkpoint"
    );
    ensure!(
      !paths.root.join("restore.hrr").exists() && !paths.root.join("generations").exists(),
      "Admission failure selected restore hardware"
    );
  }
  println!("Signed agent restore rejects an invalid checksummed schema and altered captured disk before source stop, resumes only its source pause, retains each fresh named prior checkpoint and leaves restore selection unchanged");
  Ok(())
}
