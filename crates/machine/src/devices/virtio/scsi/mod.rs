mod command;
mod optical;
mod transport;
pub use transport::execute;

use anyhow::ensure;
use std::{fs::File, path::Path};

pub const SECTOR: u64 = 2048;
pub const MAX_TRANSFER: usize = 1024 * 1024;

pub struct Optical {
  file: File,
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

pub fn open(path: &Path) -> anyhow::Result<Optical> {
  let file = File::open(path)?;
  let metadata = file.metadata()?;
  ensure!(
    metadata.is_file() && metadata.len() > 0 && metadata.len().is_multiple_of(SECTOR),
    "Optical media must be a nonempty 2048-byte sector image"
  );
  Ok(Optical {
    file,
    sectors: metadata.len() / SECTOR,
    sense_size: 96,
    cdb_size: 32,
    sense: [0; 18],
    inserted: true,
    commands: Box::new([0; 256]),
    errors: 0,
    queues: [0; 3],
    missing: None,
    control: None,
    transfer: None,
  })
}

#[derive(Debug)]
pub struct Stats {
  pub commands: Vec<(u8, u64)>,
  pub check_conditions: u64,
  pub queues: [u64; 3],
  pub last_missing: Option<[u8; 8]>,
  pub last_control: Option<(u32, u32, [u8; 8])>,
  pub last_transfer: Option<(u64, u64)>,
  pub cdb_size: u32,
  pub sense_size: u32,
}

pub fn stats(media: &Optical) -> Stats {
  Stats {
    commands: media
      .commands
      .iter()
      .enumerate()
      .filter(|(_, count)| **count != 0)
      .map(|(code, count)| (code as u8, *count))
      .collect(),
    check_conditions: media.errors,
    queues: media.queues,
    last_missing: media.missing,
    last_control: media.control,
    last_transfer: media.transfer,
    cdb_size: media.cdb_size,
    sense_size: media.sense_size,
  }
}

pub fn config(media: &Optical) -> Vec<u8> {
  let mut bytes = vec![0; 36];
  for (offset, value) in [
    (0, 1u32),
    (4, 128),
    (8, (MAX_TRANSFER / 512) as u32),
    (12, 128),
    (16, 16),
    (20, media.sense_size),
    (24, media.cdb_size),
  ] {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
  }
  bytes[30..32].copy_from_slice(&7u16.to_le_bytes());
  // older windows drivers interpret this maximum index as a count; zero hides lun 0
  bytes[32..36].copy_from_slice(&1u32.to_le_bytes());
  bytes
}

pub fn configure(media: &mut Optical, offset: usize, width: usize, value: u32) {
  match (offset, width) {
    (20, 4) if value <= 256 => media.sense_size = value,
    (24, 4) if (6..=256).contains(&value) => media.cdb_size = value,
    _ => {}
  }
}

pub fn reset(media: &mut Optical) {
  media.sense_size = 96;
  media.cdb_size = 32;
  media.sense = [0; 18];
}
