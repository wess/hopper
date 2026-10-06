//! Two interleaved 16-bit CFI NOR chips, including buffered writes and block protection.

use anyhow::{ensure, Context};
use std::ops::Range;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
  Array,
  Status,
  Query,
  Identity,
}

enum Pending {
  None,
  Word,
  Erase(usize),
  Lock(usize),
  Count(usize),
  Words {
    base: usize,
    remaining: usize,
    words: Vec<(usize, u32)>,
  },
  Confirm(Vec<(usize, u32)>),
}

pub struct Flash {
  data: Vec<u8>,
  block: usize,
  locked: Vec<bool>,
  mode: Mode,
  status: u8,
  pending: Pending,
}

pub fn create(data: Vec<u8>, block: usize) -> anyhow::Result<Flash> {
  ensure!(
    data.len().is_power_of_two() && (0x10000..=0x8000000).contains(&data.len()),
    "Flash size must be a power of two between 64 KiB and 128 MiB"
  );
  ensure!(
    block >= 512 && block.is_power_of_two() && data.len().is_multiple_of(block),
    "Flash erase blocks must divide the bank"
  );
  let locked = vec![false; data.len() / block];
  Ok(Flash {
    data,
    block,
    locked,
    mode: Mode::Array,
    status: 0x80,
    pending: Pending::None,
  })
}

pub fn bytes(flash: &Flash) -> &[u8] {
  &flash.data
}

pub fn array(flash: &Flash) -> bool {
  flash.mode == Mode::Array
}

fn query(flash: &Flash, index: usize) -> u8 {
  let blocks = flash.data.len() / flash.block - 1;
  let block = flash.block / 2 / 256;
  match index {
    0x10 => b'Q',
    0x11 => b'R',
    0x12 => b'Y',
    0x13 => 1,
    0x15 => 0x31,
    0x27 => (flash.data.len() / 2).ilog2() as u8,
    0x28 => 2,
    0x2a => 6,
    0x2c => 1,
    0x2d => blocks as u8,
    0x2e => (blocks >> 8) as u8,
    0x2f => block as u8,
    0x30 => (block >> 8) as u8,
    0x31 => b'P',
    0x32 => b'R',
    0x33 => b'I',
    0x34 => b'1',
    0x35 => b'0',
    _ => 0,
  }
}

pub fn read(flash: &Flash, offset: usize, width: u8) -> anyhow::Result<u64> {
  ensure!(
    matches!(width, 1 | 2 | 4 | 8),
    "Unsupported flash access width"
  );
  let end = offset
    .checked_add(width as usize)
    .context("Flash access overflow")?;
  ensure!(end <= flash.data.len(), "Flash read is outside its bank");
  let mut value = [0u8; 8];
  for (index, byte) in value[..width as usize].iter_mut().enumerate() {
    let address = offset + index;
    *byte = match flash.mode {
      Mode::Array => flash.data[address],
      _ if !address.is_multiple_of(2) => 0,
      Mode::Status => flash.status,
      Mode::Query => query(flash, address / 4),
      Mode::Identity => match (address % flash.block) / 4 {
        0 => 0x89,
        1 => 0x18,
        2 => flash.locked[address / flash.block].into(),
        _ => 0,
      },
    };
  }
  Ok(u64::from_le_bytes(value))
}

fn program(flash: &mut Flash, words: &[(usize, u32)]) -> Option<Range<usize>> {
  if words.iter().any(|(offset, value)| {
    flash.locked[*offset / flash.block]
      || flash.data[*offset..*offset + 4]
        .iter()
        .zip(value.to_le_bytes())
        .any(|(old, new)| old & new != new)
  }) {
    flash.status = 0x90;
    return None;
  }
  let start = words.iter().map(|(offset, _)| *offset).min()?;
  let end = words.iter().map(|(offset, _)| offset + 4).max()?;
  for (offset, value) in words {
    flash.data[*offset..*offset + 4].copy_from_slice(&value.to_le_bytes());
  }
  flash.status = 0x80;
  Some(start..end)
}

pub fn write(flash: &mut Flash, offset: usize, value: u32) -> anyhow::Result<Option<Range<usize>>> {
  ensure!(
    offset.is_multiple_of(4)
      && offset
        .checked_add(4)
        .is_some_and(|end| end <= flash.data.len()),
    "Flash write must address an aligned word in its bank"
  );
  let command = value as u8;
  let pending = std::mem::replace(&mut flash.pending, Pending::None);
  let mut changed = None;
  match pending {
    Pending::Word => changed = program(flash, &[(offset, value)]),
    Pending::Erase(block) if command == 0xd0 => {
      if flash.locked[block / flash.block] {
        flash.status = 0xa2;
      } else {
        flash.data[block..block + flash.block].fill(0xff);
        flash.status = 0x80;
        changed = Some(block..block + flash.block);
      }
    }
    Pending::Lock(block) => match command {
      1 => flash.locked[block / flash.block] = true,
      0xd0 => flash.locked[block / flash.block] = false,
      _ => flash.status = 0xb0,
    },
    Pending::Count(base) if (value & 0xffff) < 32 => {
      flash.pending = Pending::Words {
        base,
        remaining: (value & 0xffff) as usize + 1,
        words: Vec::new(),
      };
    }
    Pending::Words {
      base,
      remaining,
      mut words,
    } if offset / 128 == base / 128 && !words.iter().any(|(address, _)| *address == offset) => {
      words.push((offset, value));
      flash.pending = if remaining == 1 {
        Pending::Confirm(words)
      } else {
        Pending::Words {
          base,
          remaining: remaining - 1,
          words,
        }
      };
    }
    Pending::Confirm(words) if command == 0xd0 => changed = program(flash, &words),
    Pending::None => match command {
      0xff | 0xf0 => {
        flash.mode = Mode::Array;
        return Ok(None);
      }
      0x70 => flash.mode = Mode::Status,
      0x50 => {
        flash.status = 0x80;
        flash.mode = Mode::Status;
      }
      0x98 => {
        flash.mode = Mode::Query;
        return Ok(None);
      }
      0x90 => {
        flash.mode = Mode::Identity;
        return Ok(None);
      }
      0x40 | 0x10 => flash.pending = Pending::Word,
      0x20 => flash.pending = Pending::Erase(offset / flash.block * flash.block),
      0x60 => flash.pending = Pending::Lock(offset / flash.block * flash.block),
      0xe8 => flash.pending = Pending::Count(offset),
      _ => flash.status = 0xb0,
    },
    _ => flash.status = 0xb0,
  }
  flash.mode = Mode::Status;
  Ok(changed)
}
