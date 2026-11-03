//! HFL1 stores flash command/protection state; NVRAM bytes are a separate memory bank.

use super::{Mode, Pending, State};
use anyhow::ensure;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

pub const MAX: usize = 263000;

fn dimensions(size: usize, block: usize) -> anyhow::Result<usize> {
  ensure!(
    size.is_power_of_two()
      && (0x10000..=0x8000000).contains(&size)
      && block >= 512
      && block.is_power_of_two()
      && size.is_multiple_of(block),
    "Flash record dimensions are invalid"
  );
  Ok(size / block)
}

fn parts(pending: &Pending) -> (u8, usize, usize, &[(usize, u32)]) {
  match pending {
    Pending::None => (0, 0, 0, &[]),
    Pending::Word => (1, 0, 0, &[]),
    Pending::Erase(base) => (2, *base, 0, &[]),
    Pending::Lock(base) => (3, *base, 0, &[]),
    Pending::Count(base) => (4, *base, 0, &[]),
    Pending::Words {
      base,
      remaining,
      words,
    } => (5, *base, *remaining, words),
    Pending::Confirm(words) => (6, 0, 0, words),
  }
}

pub(super) fn validate(state: &State) -> anyhow::Result<()> {
  ensure!(
    dimensions(state.size, state.block)? == state.locked.len(),
    "Flash record protection count is invalid"
  );
  ensure!(
    matches!(state.status, 0x80 | 0x90 | 0xa2 | 0xb0),
    "Flash record status is invalid"
  );
  let (tag, base, remaining, words) = parts(&state.pending);
  ensure!(
    tag == 0 || state.mode == Mode::Status,
    "Flash pending command has incompatible mode"
  );
  let word = |address: usize| {
    address.is_multiple_of(4) && address.checked_add(4).is_some_and(|end| end <= state.size)
  };
  if matches!(tag, 2 | 3) {
    ensure!(
      base.is_multiple_of(state.block) && base < state.size,
      "Flash command block is invalid"
    );
  }
  if matches!(tag, 4 | 5) {
    ensure!(word(base), "Flash command base is invalid");
  }
  if tag == 5 {
    ensure!(
      (1..=32).contains(&remaining) && words.len() + remaining <= 32,
      "Flash buffered word count is invalid"
    );
  }
  if tag == 6 {
    ensure!(
      (1..=32).contains(&words.len()),
      "Flash confirmation has no bounded words"
    );
  }
  for (index, (address, _)) in words.iter().enumerate() {
    let window = if tag == 5 { base } else { words[0].0 } / 128;
    ensure!(
      word(*address)
        && *address / 128 == window
        && !words[..index].iter().any(|entry| entry.0 == *address),
      "Flash buffered address is invalid"
    );
  }
  Ok(())
}

pub fn encode(state: &State) -> anyhow::Result<Zeroizing<Vec<u8>>> {
  validate(state)?;
  let mut bytes = Zeroizing::new(Vec::from(b"HFL1"));
  for value in [state.size, state.block, state.locked.len()] {
    bytes.extend_from_slice(&(value as u32).to_le_bytes());
  }
  bytes.extend(state.locked.iter().map(|locked| *locked as u8));
  bytes.push(match state.mode {
    Mode::Array => 0,
    Mode::Status => 1,
    Mode::Query => 2,
    Mode::Identity => 3,
  });
  bytes.push(state.status);
  let (tag, base, remaining, words) = parts(&state.pending);
  bytes.push(tag);
  bytes.extend_from_slice(&(base as u32).to_le_bytes());
  bytes.extend_from_slice(&[remaining as u8, words.len() as u8]);
  for &(address, value) in words {
    bytes.extend_from_slice(&(address as u32).to_le_bytes());
    bytes.extend_from_slice(&value.to_le_bytes());
  }
  let checksum = Sha256::digest(&*bytes);
  bytes.extend_from_slice(&checksum);
  ensure!(bytes.len() <= MAX, "Flash record exceeds its bound");
  Ok(bytes)
}

pub fn decode(bytes: &[u8]) -> anyhow::Result<State> {
  ensure!(
    (57..=MAX).contains(&bytes.len()),
    "Flash record size is invalid"
  );
  let end = bytes.len() - 32;
  ensure!(
    Sha256::digest(&bytes[..end])[..] == bytes[end..],
    "Flash record checksum is invalid"
  );
  let mut input = Reader(&bytes[..end]);
  ensure!(
    input.take(4)? == b"HFL1",
    "Flash record format is unsupported"
  );
  let size = input.number()? as usize;
  let block = input.number()? as usize;
  let count = input.number()? as usize;
  ensure!(
    dimensions(size, block)? == count,
    "Flash record protection count is invalid"
  );
  let locked = input
    .take(count)?
    .iter()
    .map(|value| {
      ensure!(*value <= 1, "Flash protection boolean is invalid");
      Ok(*value == 1)
    })
    .collect::<anyhow::Result<Vec<_>>>()?;
  let mode = match input.byte()? {
    0 => Mode::Array,
    1 => Mode::Status,
    2 => Mode::Query,
    3 => Mode::Identity,
    _ => anyhow::bail!("Flash record mode is invalid"),
  };
  let status = input.byte()?;
  let tag = input.byte()?;
  let base = input.number()? as usize;
  let remaining = input.byte()? as usize;
  let count = input.byte()? as usize;
  ensure!(
    tag <= 6 && count <= 32,
    "Flash pending command is unsupported"
  );
  ensure!(
    (tag == 5 || remaining == 0) && (tag >= 5 || count == 0) && (matches!(tag, 2..=5) || base == 0),
    "Flash pending command fields are invalid"
  );
  let mut words = Vec::with_capacity(count);
  for _ in 0..count {
    words.push((input.number()? as usize, input.number()?));
  }
  ensure!(input.0.is_empty(), "Flash record has trailing data");
  let pending = match tag {
    0 => Pending::None,
    1 => Pending::Word,
    2 => Pending::Erase(base),
    3 => Pending::Lock(base),
    4 => Pending::Count(base),
    5 => Pending::Words {
      base,
      remaining,
      words,
    },
    _ => Pending::Confirm(words),
  };
  let state = State {
    size,
    block,
    locked,
    mode,
    status,
    pending,
  };
  validate(&state)?;
  Ok(state)
}

struct Reader<'a>(&'a [u8]);
impl<'a> Reader<'a> {
  fn take(&mut self, count: usize) -> anyhow::Result<&'a [u8]> {
    ensure!(count <= self.0.len(), "Flash record is truncated");
    let (value, rest) = self.0.split_at(count);
    self.0 = rest;
    Ok(value)
  }
  fn byte(&mut self) -> anyhow::Result<u8> {
    Ok(self.take(1)?[0])
  }
  fn number(&mut self) -> anyhow::Result<u32> {
    Ok(u32::from_le_bytes(self.take(4)?.try_into()?))
  }
}
