use super::Disk;
use crate::snapshot::storage;
use anyhow::ensure;
use fs2::FileExt;
pub mod record;

pub struct State {
  image: storage::Image,
  sectors: u64,
  readonly: bool,
  id: [u8; 20],
  writeback: bool,
}

pub fn capture(disk: &Disk) -> anyhow::Result<State> {
  ensure!(
    disk.file.metadata()?.len() == disk.sectors * 512,
    "Disk capacity changed before capture"
  );
  if !disk.readonly {
    disk.file.sync_all()?;
  }
  let image = storage::capture(&disk.file)?;
  ensure!(
    image.size() == disk.sectors * 512,
    "Disk capacity changed during capture"
  );
  Ok(State {
    image,
    sectors: disk.sectors,
    readonly: disk.readonly,
    id: disk.id,
    writeback: disk.writeback,
  })
}

/// Prepare a persistent writable system disk; publication and runtime adoption are separate.
pub fn prepare(
  state: &State,
  directory: &std::fs::File,
  authorize: impl FnMut() -> anyhow::Result<()>,
) -> anyhow::Result<storage::Prepared> {
  ensure!(
    !state.readonly && state.sectors.checked_mul(512) == Some(state.image.size()),
    "Persistent recovery requires a matching writable disk capture"
  );
  state.image.prepare(directory, authorize)
}

/// Reopen only an unchanged prepared disk from the matching captured system disk.
pub fn reopen_prepared(
  state: &State,
  directory: &std::fs::File,
  authorize: impl FnMut() -> anyhow::Result<()>,
) -> anyhow::Result<storage::Prepared> {
  ensure!(
    !state.readonly && state.sectors.checked_mul(512) == Some(state.image.size()),
    "Persistent recovery requires a matching writable disk capture"
  );
  state.image.reopen_prepared(directory, authorize)
}

/// Reconstructs into a fresh private image; the target disk is never overwritten.
/// The returned disk is ephemeral until the higher layer publishes recovery state.
pub fn restore(state: &State, target: &Disk) -> anyhow::Result<Disk> {
  ensure!(
    state.sectors == target.sectors && state.readonly == target.readonly && state.id == target.id,
    "Captured disk controller does not match the destination"
  );
  let restored = state.image.fork()?;
  restored.file.try_lock_exclusive()?;
  let mut disk = super::attach(restored.file, state.readonly, state.id)?;
  disk._temporary = Some(restored.directory);
  disk.writeback = state.writeback;
  Ok(disk)
}

pub(crate) fn check_prepared(
  state: &State,
  prepared: &storage::Prepared,
  authorize: impl FnMut() -> anyhow::Result<()>,
) -> anyhow::Result<()> {
  ensure!(
    !state.readonly,
    "Persistent restore requires a writable disk"
  );
  prepared.validate(&state.image, authorize)
}

pub(crate) fn adopt(
  state: &State,
  target: &Disk,
  prepared: storage::Prepared,
  authorize: impl FnMut() -> anyhow::Result<()>,
) -> anyhow::Result<Disk> {
  ensure!(
    state.sectors == target.sectors && state.readonly == target.readonly && state.id == target.id,
    "Persistent disk controller binding mismatch"
  );
  use std::os::unix::fs::MetadataExt;
  let target_identity = target.file.metadata()?;
  let prepared_identity = prepared.file().metadata()?;
  ensure!(
    (target_identity.dev(), target_identity.ino())
      != (prepared_identity.dev(), prepared_identity.ino()),
    "Persistent disk cannot share the replaced controller's ownership"
  );
  check_prepared(state, &prepared, authorize)?;
  Ok(Disk {
    file: prepared.into_file(),
    sectors: state.sectors,
    readonly: false,
    id: state.id,
    writeback: state.writeback,
    _temporary: None,
  })
}
