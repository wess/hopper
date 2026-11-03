use super::{Optical, SECTOR};
use crate::snapshot::storage;
use anyhow::ensure;
pub mod record;

pub struct State {
  image: storage::Image,
  sectors: u64,
  sense_size: u32,
  cdb_size: u32,
  sense: [u8; 18],
  inserted: bool,
  commands: Box<[u64; 256]>,
  errors: u64,
  queues: [u64; 3],
  missing: Option<[u8; 8]>,
  control: Option<(u32, u32, [u8; 8])>,
  transfer: Option<(u64, u64)>,
}

/// The caller holds the runtime and all device writers, including any media owner.
pub fn capture(media: &Optical) -> anyhow::Result<State> {
  ensure!(
    media.file.metadata()?.len() == media.sectors * SECTOR,
    "Optical capacity changed"
  );
  let image = storage::capture(&media.file)?;
  ensure!(
    image.size() == media.sectors * SECTOR,
    "Optical capacity changed during capture"
  );
  Ok(State {
    image,
    sectors: media.sectors,
    sense_size: media.sense_size,
    cdb_size: media.cdb_size,
    sense: media.sense,
    inserted: media.inserted,
    commands: media.commands.clone(),
    errors: media.errors,
    queues: media.queues,
    missing: media.missing,
    control: media.control,
    transfer: media.transfer,
  })
}

/// Reconstructs captured media without opening or modifying the original installer path.
pub fn restore(state: &State, target: &Optical) -> anyhow::Result<Optical> {
  ensure!(
    state.sectors == target.sectors,
    "Captured optical capacity does not match destination"
  );
  let restored = state.image.fork_readonly()?;
  let mut media = super::attach(restored.file)?;
  media._temporary = Some(restored.directory);
  media.sense_size = state.sense_size;
  media.cdb_size = state.cdb_size;
  media.sense = state.sense;
  media.inserted = state.inserted;
  media.commands = state.commands.clone();
  media.errors = state.errors;
  media.queues = state.queues;
  media.missing = state.missing;
  media.control = state.control;
  media.transfer = state.transfer;
  Ok(media)
}
