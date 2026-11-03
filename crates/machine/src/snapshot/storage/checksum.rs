use super::extent::{self, CHUNK};
use anyhow::ensure;
use sha2::{Digest, Sha256};
use std::{fs::File, os::unix::fs::FileExt};

/// HDS2 binds size and ordered nonzero 64 KiB chunks; allocated zeros and holes hash identically.
pub(super) fn sparse(
  file: &File,
  size: u64,
  authorize: &mut impl FnMut() -> anyhow::Result<()>,
) -> anyhow::Result<[u8; 32]> {
  authorize()?;
  ensure!(
    file.metadata()?.len() == size,
    "Snapshot disk capacity changed"
  );
  let mut digest = Sha256::new();
  digest.update(b"hopper.snapshot.disk.v2\0");
  digest.update(size.to_le_bytes());
  let mut bytes = [0; CHUNK as usize];
  let zeros = [0; CHUNK as usize];
  let mut offset = 0;
  while offset < size {
    authorize()?;
    offset = extent::next(file, offset, size)?;
    authorize()?;
    if offset == size {
      break;
    }
    let count = (size - offset).min(CHUNK) as usize;
    file.read_exact_at(&mut bytes[..count], offset)?;
    authorize()?;
    if bytes[..count] != zeros[..count] {
      digest.update(offset.to_le_bytes());
      digest.update((count as u32).to_le_bytes());
      digest.update(Sha256::digest(&bytes[..count]));
    }
    offset += count as u64;
    authorize()?;
  }
  ensure!(
    file.metadata()?.len() == size,
    "Snapshot disk changed during hashing"
  );
  authorize()?;
  Ok(digest.finalize().into())
}
