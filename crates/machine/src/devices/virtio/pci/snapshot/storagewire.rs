//! storage records reopen independently of original captured image ownership.

use super::{backend, State};
use crate::devices::virtio::{
  block::snapshot::record as diskrecord, scsi::snapshot::record as opticalrecord,
};
use anyhow::Context;
use std::fs::File;

pub const MAX: usize = opticalrecord::SIZE;

pub fn present(saved: &State) -> bool {
  matches!(
    saved.backend,
    Some(backend::State::Disk(_) | backend::State::Optical(_))
  )
}

pub fn save(
  saved: &State,
  directory: &File,
  authorize: impl FnMut() -> anyhow::Result<()>,
) -> anyhow::Result<Vec<u8>> {
  match &saved.backend {
    Some(backend::State::Disk(disk)) => Ok(diskrecord::save(disk, directory, authorize)?.to_vec()),
    Some(backend::State::Optical(media)) => {
      Ok(opticalrecord::save(media, directory, authorize)?.to_vec())
    }
    _ => anyhow::bail!("Storage record is unavailable"),
  }
}

pub fn reopen(
  mut saved: State,
  directory: &File,
  bytes: &[u8],
  authorize: impl FnMut() -> anyhow::Result<()>,
) -> anyhow::Result<State> {
  let original = saved
    .backend
    .take()
    .context("Storage record is unavailable")?;
  let reopened = match original {
    backend::State::Disk(disk) => {
      let expected = diskrecord::binding(&disk);
      drop(disk);
      backend::State::Disk(diskrecord::load(directory, bytes, expected, authorize)?)
    }
    backend::State::Optical(media) => {
      let expected = opticalrecord::sectors(&media);
      drop(media);
      backend::State::Optical(opticalrecord::load(directory, bytes, expected, authorize)?)
    }
    _ => anyhow::bail!("Storage record is unavailable"),
  };
  saved.backend = Some(reopened);
  Ok(saved)
}
