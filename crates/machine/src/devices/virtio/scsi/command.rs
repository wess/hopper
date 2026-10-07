use super::{Optical, MAX_TRANSFER, SECTOR};
use std::io::{Read, Seek, SeekFrom};

pub(super) struct Reply {
  pub data: Vec<u8>,
  pub sense: Option<[u8; 18]>,
}

fn good(data: Vec<u8>) -> Reply {
  Reply { data, sense: None }
}

pub(super) fn error(media: &mut Optical, key: u8, asc: u8) -> Reply {
  media.errors += 1;
  let mut sense = [0; 18];
  sense[0] = 0x70;
  sense[2] = key;
  sense[7] = 10;
  sense[12] = asc;
  media.sense = sense;
  Reply {
    data: Vec::new(),
    sense: Some(sense),
  }
}

fn word(cdb: &[u8], offset: usize) -> u16 {
  u16::from_be_bytes([cdb[offset], cdb[offset + 1]])
}
fn dword(cdb: &[u8], offset: usize) -> u32 {
  u32::from_be_bytes(cdb[offset..offset + 4].try_into().unwrap())
}

pub(super) fn execute(media: &mut Optical, cdb: &[u8]) -> anyhow::Result<Reply> {
  let length = match cdb.first().map(|code| code >> 5) {
    Some(0) => 6,
    Some(1 | 2) => 10,
    Some(4) => 16,
    Some(5) => 12,
    _ => 1,
  };
  if cdb.len() < length {
    return Ok(error(media, 5, 0x24));
  }
  media.commands[cdb[0] as usize] += 1;
  if !media.inserted
    && !matches!(
      cdb[0],
      0x03 | 0x12 | 0x1a | 0x1b | 0x46 | 0x4a | 0x5a | 0xa0
    )
  {
    return Ok(error(media, 2, 0x3a));
  }
  let data = match cdb[0] {
    0x00 | 0x1e | 0x35 => Vec::new(),
    0x1b => {
      if cdb[4] & 2 != 0 {
        media.inserted = cdb[4] & 1 != 0;
      }
      Vec::new()
    }
    0x03 => {
      let mut sense = media.sense;
      if sense[0] == 0 {
        sense[0] = 0x70;
        sense[7] = 10;
      }
      media.sense = [0; 18];
      sense[..(cdb[4] as usize).min(18)].to_vec()
    }
    0x12 => {
      let bytes = if cdb[1] & 1 != 0 {
        if cdb[2] != 0 {
          return Ok(error(media, 5, 0x24));
        }
        vec![5, 0, 0, 1, 0]
      } else {
        if cdb[2] != 0 {
          return Ok(error(media, 5, 0x24));
        }
        let mut bytes = vec![0; 36];
        bytes[..5].copy_from_slice(&[5, 0x80, 5, 2, 31]);
        bytes[8..16].copy_from_slice(b"HOPPER  ");
        bytes[16..32].copy_from_slice(b"Virtual CD-ROM  ");
        bytes[32..36].copy_from_slice(b"0001");
        bytes
      };
      bytes[..bytes.len().min(word(cdb, 3) as usize)].to_vec()
    }
    0x25 => {
      let mut bytes = ((media.sectors - 1).min(u32::MAX as u64) as u32)
        .to_be_bytes()
        .to_vec();
      bytes.extend((SECTOR as u32).to_be_bytes());
      bytes
    }
    0x9e if cdb[1] & 0x1f == 0x10 => {
      let mut bytes = vec![0; 32];
      bytes[..8].copy_from_slice(&(media.sectors - 1).to_be_bytes());
      bytes[8..12].copy_from_slice(&(SECTOR as u32).to_be_bytes());
      bytes.truncate((dword(cdb, 10) as usize).min(32));
      bytes
    }
    0xa0 => {
      if cdb[2] > 2 {
        return Ok(error(media, 5, 0x24));
      }
      let mut bytes = vec![0; 16];
      bytes[3] = 8;
      bytes.truncate((dword(cdb, 6) as usize).min(16));
      bytes
    }
    0x1a | 0x5a | 0x43 | 0x46 | 0x4a => {
      return Ok(super::optical::execute(media, cdb));
    }
    0x28 | 0xa8 | 0x88 => {
      let (sector, count) = match cdb[0] {
        0x28 => (dword(cdb, 2) as u64, word(cdb, 7) as u64),
        0xa8 => (dword(cdb, 2) as u64, dword(cdb, 6) as u64),
        _ => (
          u64::from_be_bytes(cdb[2..10].try_into().unwrap()),
          dword(cdb, 10) as u64,
        ),
      };
      if count > (MAX_TRANSFER as u64 / SECTOR) {
        return Ok(error(media, 5, 0x24));
      }
      if sector
        .checked_add(count)
        .is_none_or(|end| end > media.sectors)
      {
        return Ok(error(media, 5, 0x21));
      }
      let mut bytes = vec![0; (count * SECTOR) as usize];
      media.file.seek(SeekFrom::Start(sector * SECTOR))?;
      media.file.read_exact(&mut bytes)?;
      bytes
    }
    0x0a | 0x2a | 0x8a | 0xaa | 0x04 | 0x15 | 0x55 => return Ok(error(media, 7, 0x27)),
    _ => return Ok(error(media, 5, 0x20)),
  };
  Ok(good(data))
}
