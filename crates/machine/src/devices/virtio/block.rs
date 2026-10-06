use super::queue::Chain;
use crate::dma::{self, Memory, Span};
use anyhow::{ensure, Context};
use std::{
  fs::{File, OpenOptions},
  io::{Read, Seek, SeekFrom, Write},
  path::Path,
};

pub struct Disk {
  file: File,
  sectors: u64,
  readonly: bool,
  id: [u8; 20],
  writeback: bool,
}

pub fn open(path: &Path, readonly: bool, id: [u8; 20]) -> anyhow::Result<Disk> {
  ensure!(id.is_ascii(), "Virtio disk identity must be ASCII");
  let file = OpenOptions::new()
    .read(true)
    .write(!readonly)
    .open(path)
    .with_context(|| format!("Open VM disk {}", path.display()))?;
  let metadata = file.metadata()?;
  ensure!(
    metadata.is_file() && metadata.len() > 0 && metadata.len().is_multiple_of(512),
    "Virtio disk must be a nonempty sector-aligned file"
  );
  Ok(Disk {
    file,
    sectors: metadata.len() / 512,
    readonly,
    id,
    writeback: false,
  })
}

pub fn capacity(disk: &Disk) -> u64 {
  disk.sectors
}
pub fn readonly(disk: &Disk) -> bool {
  disk.readonly
}

pub fn writeback(disk: &mut Disk, enabled: bool) -> anyhow::Result<()> {
  if disk.writeback && !enabled {
    disk.file.sync_all()?;
  }
  disk.writeback = enabled;
  Ok(())
}

pub fn execute(disk: &mut Disk, memory: &mut impl Memory, chain: &Chain) -> anyhow::Result<u32> {
  let readable: Vec<Span> = chain
    .buffers
    .iter()
    .filter(|buffer| !buffer.writable)
    .map(|buffer| buffer.span)
    .collect();
  let writable: Vec<Span> = chain
    .buffers
    .iter()
    .filter(|buffer| buffer.writable)
    .map(|buffer| buffer.span)
    .collect();
  ensure!(
    dma::length(&readable) >= 16 && dma::length(&writable) > 0,
    "Virtio block request has no header or status buffer"
  );
  let mut header = [0; 16];
  dma::read(memory, &readable, 0, &mut header)?;
  let kind = u32::from_le_bytes(header[..4].try_into()?);
  let sector = u64::from_le_bytes(header[8..].try_into()?);
  let mut written = 0u32;
  let status = match kind {
    0 | 1 => {
      let length = if kind == 0 {
        dma::length(&writable) - 1
      } else {
        dma::length(&readable) - 16
      };
      if !length.is_multiple_of(512)
        || sector > disk.sectors
        || length / 512 > disk.sectors - sector
        || (kind == 1 && disk.readonly)
      {
        1
      } else {
        let result = transfer(
          disk,
          memory,
          if kind == 0 { &writable } else { &readable },
          sector,
          length,
          kind == 1,
          &mut written,
        );
        u8::from(result.is_err())
      }
    }
    4 => {
      if disk.readonly {
        0
      } else {
        u8::from(disk.file.sync_all().is_err())
      }
    }
    8 => {
      if dma::length(&writable) < 21 {
        1
      } else {
        dma::write(memory, &writable, 0, &disk.id)?;
        written = 20;
        0
      }
    }
    _ => 2,
  };
  dma::write(memory, &writable, dma::length(&writable) - 1, &[status])?;
  Ok(written + 1)
}

fn transfer(
  disk: &mut Disk,
  memory: &mut impl Memory,
  spans: &[Span],
  sector: u64,
  length: u64,
  write: bool,
  written: &mut u32,
) -> anyhow::Result<()> {
  disk.file.seek(SeekFrom::Start(
    sector.checked_mul(512).context("Disk offset overflow")?,
  ))?;
  let mut bytes = [0; 65536];
  let mut offset = 0;
  while offset < length {
    let count = (length - offset).min(bytes.len() as u64) as usize;
    let bytes = &mut bytes[..count];
    if write {
      dma::read(memory, spans, 16 + offset, bytes)?;
      disk.file.write_all(bytes)?;
    } else {
      disk.file.read_exact(bytes)?;
      dma::write(memory, spans, offset, bytes)?;
      *written += count as u32;
    }
    offset += count as u64;
  }
  if write && !disk.writeback {
    disk.file.sync_all()?;
  }
  Ok(())
}
