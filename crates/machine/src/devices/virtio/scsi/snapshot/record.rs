//! optical controller metadata and independently staged captured media.

use super::{storage, Optical, State, SECTOR};
use anyhow::ensure;
use std::fs::File;
mod codec;
pub const SIZE: usize = 2270;

pub fn sectors(saved: &State) -> u64 {
  saved.sectors
}

pub fn target(media: &Optical) -> u64 {
  media.sectors
}

pub fn save(
  saved: &State,
  directory: &File,
  mut authorize: impl FnMut() -> anyhow::Result<()>,
) -> anyhow::Result<[u8; SIZE]> {
  ensure!(
    saved.sectors.checked_mul(SECTOR) == Some(saved.image.size()),
    "Optical record capacity is inconsistent"
  );
  let image = storage::record::save(&saved.image, directory, &mut authorize)?;
  let bytes = codec::encode(saved, &image)?;
  authorize()?;
  Ok(bytes)
}

pub fn load(
  directory: &File,
  bytes: &[u8],
  expected: u64,
  mut authorize: impl FnMut() -> anyhow::Result<()>,
) -> anyhow::Result<State> {
  authorize()?;
  let saved = codec::decode(bytes, expected)?;
  let size = expected
    .checked_mul(SECTOR)
    .ok_or_else(|| anyhow::anyhow!("Optical capacity overflow"))?;
  let image = storage::record::load(directory, &bytes[2162..2238], size, &mut authorize)?;
  authorize()?;
  Ok(State {
    image,
    sectors: expected,
    sense_size: saved.sense_size,
    cdb_size: saved.cdb_size,
    sense: saved.sense,
    inserted: saved.inserted,
    commands: saved.commands,
    errors: saved.errors,
    queues: saved.queues,
    missing: saved.missing,
    control: saved.control,
    transfer: saved.transfer,
  })
}
