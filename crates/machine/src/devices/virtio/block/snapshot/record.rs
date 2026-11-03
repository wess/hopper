//! disk identity/controller metadata with a separately staged immutable image.

use super::{storage, State};
use anyhow::ensure;
use sha2::{Digest, Sha256};
use std::fs::File;

pub const SIZE: usize = 142;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Binding {
  sectors: u64,
  readonly: bool,
  id: [u8; 20],
}

pub fn binding(saved: &State) -> Binding {
  Binding {
    sectors: saved.sectors,
    readonly: saved.readonly,
    id: saved.id,
  }
}

pub fn target(disk: &super::super::Disk) -> Binding {
  Binding {
    sectors: disk.sectors,
    readonly: disk.readonly,
    id: disk.id,
  }
}

pub fn save(
  saved: &State,
  directory: &File,
  mut authorize: impl FnMut() -> anyhow::Result<()>,
) -> anyhow::Result<[u8; SIZE]> {
  ensure!(
    !saved.readonly || !saved.writeback,
    "Read-only disk cannot enable writeback"
  );
  ensure!(
    saved.sectors.checked_mul(512) == Some(saved.image.size()),
    "Disk record capacity is inconsistent"
  );
  let image = storage::record::save(&saved.image, directory, &mut authorize)?;
  let mut bytes = [0; SIZE];
  bytes[..4].copy_from_slice(b"HDB1");
  bytes[4..12].copy_from_slice(&saved.sectors.to_le_bytes());
  bytes[12] = saved.readonly as u8;
  bytes[13] = saved.writeback as u8;
  bytes[14..34].copy_from_slice(&saved.id);
  bytes[34..110].copy_from_slice(&image);
  let checksum = Sha256::digest(&bytes[..110]);
  bytes[110..].copy_from_slice(&checksum);
  authorize()?;
  Ok(bytes)
}

pub fn load(
  directory: &File,
  bytes: &[u8],
  expected: Binding,
  mut authorize: impl FnMut() -> anyhow::Result<()>,
) -> anyhow::Result<State> {
  authorize()?;
  let binding = inspect(bytes)?;
  ensure!(
    binding == expected,
    "Disk controller record binding mismatch"
  );
  let size = binding
    .sectors
    .checked_mul(512)
    .ok_or_else(|| anyhow::anyhow!("Disk controller capacity overflow"))?;
  let image = storage::record::load(directory, &bytes[34..110], size, &mut authorize)?;
  authorize()?;
  Ok(State {
    image,
    sectors: binding.sectors,
    readonly: binding.readonly,
    id: binding.id,
    writeback: bytes[13] == 1,
  })
}

fn inspect(bytes: &[u8]) -> anyhow::Result<Binding> {
  ensure!(
    bytes.len() == SIZE && &bytes[..4] == b"HDB1",
    "Unknown or malformed disk controller record"
  );
  ensure!(
    Sha256::digest(&bytes[..110])[..] == bytes[110..],
    "Disk controller record checksum mismatch"
  );
  ensure!(
    bytes[12] <= 1 && bytes[13] <= 1,
    "Disk controller record boolean is invalid"
  );
  ensure!(
    bytes[12] == 0 || bytes[13] == 0,
    "Read-only disk cannot enable writeback"
  );
  let binding = Binding {
    sectors: u64::from_le_bytes(bytes[4..12].try_into()?),
    readonly: bytes[12] == 1,
    id: bytes[14..34].try_into()?,
  };
  let size = binding
    .sectors
    .checked_mul(512)
    .ok_or_else(|| anyhow::anyhow!("Disk controller capacity overflow"))?;
  storage::record::validate(&bytes[34..110], size)?;
  Ok(binding)
}

/// Empty hardware shape only, for fingerprint/component validation before recovered disks attach.
/// Never execute a guest on this disk; it contains no checkpoint data.
pub fn prototype(bytes: &[u8], id: [u8; 20]) -> anyhow::Result<super::super::Disk> {
  let binding = inspect(bytes)?;
  ensure!(
    !binding.readonly && binding.id == id,
    "Recovery disk identity/mode differs"
  );
  let file = tempfile::tempfile()?;
  file.set_len(binding.sectors * 512)?;
  super::super::attach(file, false, id)
}
