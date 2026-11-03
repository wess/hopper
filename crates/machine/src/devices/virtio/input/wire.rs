//! HIN1 stores already-normalized device input, never host input ownership.

use super::{Event, Input, Kind, SYN};
use anyhow::ensure;
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, VecDeque};
use zeroize::Zeroizing;

pub const MAX: usize = 9000;

fn key(kind: Kind, code: u16) -> bool {
  match kind {
    Kind::Keyboard => (1..=255).contains(&code),
    Kind::Tablet => (272..=274).contains(&code),
  }
}

fn validate(input: &Input) -> anyhow::Result<()> {
  ensure!(
    input.pending.len() <= 1024
      && input.held.len() <= 255
      && input.held.iter().all(|code| key(input.kind, *code))
      && input.leds <= 7
      && (input.kind == Kind::Keyboard || input.leds == 0),
    "Input record dimensions or held state are invalid"
  );
  ensure!(
    input.pending.is_empty() || input.pending.back() == Some(&SYN),
    "Input record lacks final SYN_REPORT"
  );
  for event in &input.pending {
    let valid = match (event.kind, input.kind) {
      (0, _) => *event == SYN,
      (1, Kind::Keyboard) => key(input.kind, event.code) && (0..=2).contains(&event.value),
      (1, Kind::Tablet) => key(input.kind, event.code) && (0..=1).contains(&event.value),
      (3, Kind::Tablet) => event.code <= 1 && (0..=32767).contains(&event.value),
      (2, Kind::Tablet) => event.code == 8,
      _ => false,
    };
    ensure!(valid, "Input record event is invalid");
  }
  Ok(())
}

pub fn encode(input: &Input) -> anyhow::Result<Zeroizing<Vec<u8>>> {
  validate(input)?;
  let mut bytes = Zeroizing::new(Vec::from(b"HIN1"));
  bytes.extend_from_slice(&[
    match input.kind {
      Kind::Keyboard => 1,
      Kind::Tablet => 2,
    },
    input.select,
    input.subselect,
    input.leds,
  ]);
  bytes.extend_from_slice(&(input.pending.len() as u16).to_le_bytes());
  for event in &input.pending {
    bytes.extend_from_slice(&event.kind.to_le_bytes());
    bytes.extend_from_slice(&event.code.to_le_bytes());
    bytes.extend_from_slice(&event.value.to_le_bytes());
  }
  bytes.extend_from_slice(&(input.held.len() as u16).to_le_bytes());
  for code in &input.held {
    bytes.extend_from_slice(&code.to_le_bytes());
  }
  let checksum = Sha256::digest(&*bytes);
  bytes.extend_from_slice(&checksum);
  Ok(bytes)
}

pub fn decode(bytes: &[u8]) -> anyhow::Result<Input> {
  ensure!(
    (44..=MAX).contains(&bytes.len()),
    "Input record size is invalid"
  );
  let end = bytes.len() - 32;
  ensure!(
    Sha256::digest(&bytes[..end])[..] == bytes[end..],
    "Input record checksum is invalid"
  );
  ensure!(&bytes[..4] == b"HIN1", "Input record format is unsupported");
  let kind = match bytes[4] {
    1 => Kind::Keyboard,
    2 => Kind::Tablet,
    _ => anyhow::bail!("Input record kind is unsupported"),
  };
  let count = u16::from_le_bytes(bytes[8..10].try_into()?) as usize;
  ensure!(
    count <= 1024 && 12 + count * 8 <= end,
    "Input record event count is invalid"
  );
  let mut pending = VecDeque::with_capacity(count);
  for index in 0..count {
    let offset = 10 + index * 8;
    pending.push_back(Event {
      kind: u16::from_le_bytes(bytes[offset..offset + 2].try_into()?),
      code: u16::from_le_bytes(bytes[offset + 2..offset + 4].try_into()?),
      value: i32::from_le_bytes(bytes[offset + 4..offset + 8].try_into()?),
    });
  }
  let offset = 10 + count * 8;
  let count = u16::from_le_bytes(bytes[offset..offset + 2].try_into()?) as usize;
  ensure!(
    count <= 255 && offset + 2 + count * 2 == end,
    "Input record held count is invalid"
  );
  let mut held = BTreeSet::new();
  for index in 0..count {
    let offset = offset + 2 + index * 2;
    let code = u16::from_le_bytes(bytes[offset..offset + 2].try_into()?);
    ensure!(
      held.last().is_none_or(|last| *last < code),
      "Input record held codes are not unique and ordered"
    );
    held.insert(code);
  }
  let input = Input {
    kind,
    select: bytes[5],
    subselect: bytes[6],
    leds: bytes[7],
    pending,
    held,
  };
  validate(&input)?;
  Ok(input)
}
