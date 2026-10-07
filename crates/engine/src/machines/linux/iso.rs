use anyhow::{ensure, Context};
use machine::vz::queue::Check;
use std::io::{Read, Seek, SeekFrom};

const SECTOR: u64 = 2048;

pub fn read(
  input: &mut (impl Read + Seek),
  path: &str,
  maximum: usize,
  check: &Check,
) -> anyhow::Result<Vec<u8>> {
  let (offset, size) = locate(input, path, maximum, check)?;
  let length = input.seek(SeekFrom::End(0))?;
  range(input, offset, size, length, check)
}

pub fn locate(
  input: &mut (impl Read + Seek),
  path: &str,
  maximum: usize,
  check: &Check,
) -> anyhow::Result<(u64, usize)> {
  check()?;
  ensure!(
    (1..=512 << 20).contains(&maximum),
    "ISO asset limit exceeds bounds"
  );
  let parts: Vec<_> = path.split('/').collect();
  ensure!(
    (1..=4).contains(&parts.len())
      && parts
        .iter()
        .all(|part| !part.is_empty() && *part != "." && *part != ".." && part.is_ascii()),
    "Invalid installer asset path"
  );
  let length = input.seek(SeekFrom::End(0))?;
  ensure!(
    (17 * SECTOR..=16 << 30).contains(&length),
    "ISO media exceeds bounds"
  );
  let descriptor = range(input, 16 * SECTOR, SECTOR as usize, length, check)?;
  ensure!(
    descriptor[..7] == *b"\x01CD001\x01" && descriptor[128..132] == [0, 8, 8, 0],
    "Installer requires ISO9660 with 2048-byte sectors"
  );
  let volume = u64::from(number(&descriptor[80..88])?) * SECTOR;
  ensure!(
    volume <= length && volume >= 17 * SECTOR,
    "ISO volume exceeds media bounds"
  );
  let (mut offset, mut size, mut directory) = extent(&descriptor[156..190], volume)?;
  for (index, part) in parts.iter().enumerate() {
    ensure!(
      directory && (1..=4 << 20).contains(&size),
      "Installer directory exceeds bounds"
    );
    let entries = range(input, offset, size, volume, check)?;
    let mut cursor = 0;
    let mut found = None;
    let mut count = 0;
    while cursor < entries.len() {
      let size = entries[cursor] as usize;
      if size == 0 {
        cursor = ((cursor / SECTOR as usize) + 1) * SECTOR as usize;
        continue;
      }
      count += 1;
      ensure!(
        count <= 8192 && size >= 34 && cursor + size <= entries.len(),
        "Invalid ISO directory record"
      );
      let entry = &entries[cursor..cursor + size];
      ensure!(
        cursor % SECTOR as usize + size <= SECTOR as usize,
        "ISO record crosses a sector"
      );
      let namesize = entry[32] as usize;
      ensure!(
        namesize > 0 && 33 + namesize <= size,
        "Invalid ISO filename"
      );
      let name = &entry[33..33 + namesize];
      if name != [0] && name != [1] {
        let name = ridge(entry, namesize).unwrap_or(name);
        let name = std::str::from_utf8(name).context("ISO filename must be UTF-8")?;
        let name = name.split(';').next().unwrap_or(name).trim_end_matches('.');
        if name.eq_ignore_ascii_case(part) {
          ensure!(found.is_none(), "Ambiguous installer asset");
          found = Some(extent(entry, volume)?);
        }
      }
      cursor += size;
    }
    (offset, size, directory) = found.context("Required installer asset is missing")?;
    if index + 1 == parts.len() {
      ensure!(
        !directory && size > 0 && size <= maximum,
        "Installer asset exceeds bounds"
      );
    }
  }
  check()?;
  Ok((offset, size))
}

fn ridge(record: &[u8], namesize: usize) -> Option<&[u8]> {
  let mut cursor = (33 + namesize).next_multiple_of(2);
  while cursor + 4 <= record.len() {
    let size = record[cursor + 2] as usize;
    if size < 4 || cursor + size > record.len() {
      return None;
    }
    if &record[cursor..cursor + 2] == b"NM"
      && size >= 5
      && record[cursor + 3] == 1
      && record[cursor + 4] == 0
    {
      return Some(&record[cursor + 5..cursor + size]);
    }
    cursor += size;
  }
  None
}

fn extent(record: &[u8], volume: u64) -> anyhow::Result<(u64, usize, bool)> {
  ensure!(
    record.len() >= 34 && record[1] == 0 && record[25] & 0x80 == 0,
    "Unsupported ISO asset extent"
  );
  let offset = u64::from(number(&record[2..10])?) * SECTOR;
  let size = number(&record[10..18])? as usize;
  ensure!(
    offset >= 17 * SECTOR && offset + size as u64 <= volume,
    "ISO extent exceeds volume bounds"
  );
  Ok((offset, size, record[25] & 2 != 0))
}

fn number(bytes: &[u8]) -> anyhow::Result<u32> {
  let value = u32::from_le_bytes(bytes[..4].try_into()?);
  ensure!(
    value == u32::from_be_bytes(bytes[4..8].try_into()?),
    "ISO byte orders disagree"
  );
  Ok(value)
}

fn range(
  input: &mut (impl Read + Seek),
  offset: u64,
  size: usize,
  length: u64,
  check: &Check,
) -> anyhow::Result<Vec<u8>> {
  ensure!(offset + size as u64 <= length, "ISO read exceeds bounds");
  check()?;
  input.seek(SeekFrom::Start(offset))?;
  let mut bytes = vec![0; size];
  for chunk in bytes.chunks_mut(64 << 10) {
    check()?;
    input.read_exact(chunk)?;
  }
  check()?;
  Ok(bytes)
}
