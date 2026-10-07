use super::{COMPACT, SIZE};
use anyhow::ensure;
use sha2::{Digest, Sha256};
use std::{
  fs::File,
  io::{Read, Seek, SeekFrom},
};

const MAGIC: &[u8; 8] = b"HOPVAR01";
const COMMIT: &[u8; 8] = b"COMMIT01";
const LIMIT: usize = 0x40000;
const HEADER: usize = 56;
const FOOTER: usize = 40;

pub(super) fn validate(offset: usize, length: usize) -> anyhow::Result<()> {
  ensure!(
    offset.is_multiple_of(4)
      && length > 0
      && length <= LIMIT
      && length.is_multiple_of(4)
      && offset.checked_add(length).is_some_and(|end| end <= SIZE),
    "Invalid variable change range"
  );
  Ok(())
}

pub(super) fn record(sequence: u64, offset: usize, bytes: &[u8]) -> anyhow::Result<Vec<u8>> {
  validate(offset, bytes.len())?;
  let mut record = Vec::with_capacity(HEADER + bytes.len() + FOOTER);
  record.extend(MAGIC);
  record.extend(sequence.to_le_bytes());
  record.extend((offset as u32).to_le_bytes());
  record.extend((bytes.len() as u32).to_le_bytes());
  let checksum = Sha256::digest(&record);
  record.extend(checksum);
  record.extend(bytes);
  record.extend(Sha256::digest(bytes));
  record.extend(COMMIT);
  Ok(record)
}

pub(super) fn replay(file: &mut File, bank: &mut [u8], sequence: &mut u64) -> anyhow::Result<u64> {
  ensure!(
    file.metadata()?.len() <= COMPACT + (HEADER + LIMIT + FOOTER) as u64,
    "Variable journal exceeds its bound"
  );
  file.seek(SeekFrom::Start(0))?;
  let mut bytes = Vec::new();
  file
    .take(COMPACT + (HEADER + LIMIT + FOOTER) as u64 + 1)
    .read_to_end(&mut bytes)?;
  ensure!(
    bytes.len() as u64 <= COMPACT + (HEADER + LIMIT + FOOTER) as u64,
    "Variable journal exceeds its bound"
  );
  let mut cursor = 0;
  let mut previous: Option<u64> = None;
  while bytes.len() - cursor >= HEADER {
    let header = &bytes[cursor..cursor + HEADER];
    ensure!(
      &header[..8] == MAGIC && Sha256::digest(&header[..24])[..] == header[24..],
      "Corrupt variable journal header"
    );
    let current = u64::from_le_bytes(header[8..16].try_into()?);
    ensure!(
      current > 0
        && match previous {
          Some(value) => Some(current) == value.checked_add(1),
          None => current <= sequence.saturating_add(1),
        },
      "Invalid variable journal sequence"
    );
    let offset = u32::from_le_bytes(header[16..20].try_into()?) as usize;
    let length = u32::from_le_bytes(header[20..24].try_into()?) as usize;
    validate(offset, length)?;
    let end = cursor + HEADER + length + FOOTER;
    if end > bytes.len() {
      break;
    }
    let data = &bytes[cursor + HEADER..cursor + HEADER + length];
    let footer = &bytes[cursor + HEADER + length..end];
    ensure!(
      Sha256::digest(data)[..] == footer[..32] && &footer[32..] == COMMIT,
      "Corrupt variable journal commit"
    );
    if current > *sequence {
      ensure!(
        current == sequence.saturating_add(1),
        "Variable journal sequence gap"
      );
      bank[offset..offset + length].copy_from_slice(data);
      *sequence = current;
    }
    previous = Some(current);
    cursor = end;
  }
  Ok(cursor as u64)
}
