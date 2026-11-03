//! HSW1 preserves native CPU power/debug topology and relative local boot actions.

use super::{actions, Software};
use crate::{debug, psci::power};
use anyhow::ensure;
use sha2::{Digest, Sha256};
use std::time::Duration;
use zeroize::Zeroizing;

pub const MAX: usize = 512;

pub struct State {
  pub software: Software,
  pub actions: actions::State,
}

fn validate(software: &Software, actions: &actions::State) -> anyhow::Result<()> {
  let saved = &software.power;
  ensure!(
    (1..=2).contains(&saved.cores.len())
      && software.debug.len() == saved.cores.len()
      && (1..=16).contains(&saved.executable.len()),
    "Software record dimensions are unsupported"
  );
  ensure!(
    saved.cores[0].1
      && saved
        .cores
        .iter()
        .all(|(affinity, _)| affinity & !0xff00ffffff == 0),
    "Software record CPU state is invalid"
  );
  let affinities = saved.cores.iter().map(|core| core.0).collect::<Vec<_>>();
  power::create(&affinities, &saved.executable)?;
  ensure!(
    actions.tail.len() <= 64
      && actions
        .enter
        .is_none_or(|delay| delay <= Duration::from_secs(1)),
    "Software record local actions exceed their bound"
  );
  Ok(())
}

pub fn encode(software: &Software, actions: &actions::State) -> anyhow::Result<Zeroizing<Vec<u8>>> {
  validate(software, actions)?;
  let mut bytes = Zeroizing::new(Vec::from(b"HSW1"));
  bytes.push(software.power.cores.len() as u8);
  for ((affinity, online), debug) in software.power.cores.iter().zip(&software.debug) {
    bytes.extend_from_slice(&affinity.to_le_bytes());
    bytes.extend_from_slice(&[*online as u8, debug.locked as u8]);
  }
  bytes.push(software.power.executable.len() as u8);
  for range in &software.power.executable {
    bytes.extend_from_slice(&range.start.to_le_bytes());
    bytes.extend_from_slice(&range.end.to_le_bytes());
  }
  bytes.push(actions.enter.is_some() as u8);
  bytes.extend_from_slice(&(actions.enter.unwrap_or_default().as_nanos() as u64).to_le_bytes());
  bytes.push(actions.tail.len() as u8);
  bytes.extend_from_slice(&actions.tail);
  let checksum = Sha256::digest(&*bytes);
  bytes.extend_from_slice(&checksum);
  ensure!(bytes.len() <= MAX, "Software record exceeds its bound");
  Ok(bytes)
}

pub fn decode(bytes: &[u8]) -> anyhow::Result<State> {
  ensure!(
    (32..=MAX).contains(&bytes.len()),
    "Software record size is invalid"
  );
  let end = bytes.len() - 32;
  ensure!(
    Sha256::digest(&bytes[..end])[..] == bytes[end..],
    "Software record checksum is invalid"
  );
  let mut input = Reader(&bytes[..end]);
  ensure!(
    input.take(4)? == b"HSW1",
    "Software record format is unsupported"
  );
  let count = input.byte()? as usize;
  ensure!(
    (1..=2).contains(&count),
    "Software record CPU count is unsupported"
  );
  let mut cores = Vec::with_capacity(count);
  let mut debug = Vec::with_capacity(count);
  for _ in 0..count {
    cores.push((input.number()?, input.boolean()?));
    debug.push(debug::State {
      locked: input.boolean()?,
    });
  }
  let count = input.byte()? as usize;
  ensure!(
    (1..=16).contains(&count),
    "Software record region count is unsupported"
  );
  let mut executable = Vec::with_capacity(count);
  for _ in 0..count {
    executable.push(input.number()?..input.number()?);
  }
  let present = input.boolean()?;
  let nanos = input.number()?;
  ensure!(present || nanos == 0, "Absent software action has a delay");
  let count = input.byte()? as usize;
  ensure!(count <= 64, "Software record UART tail exceeds its bound");
  let actions = actions::State {
    tail: input.take(count)?.to_vec(),
    enter: present.then(|| Duration::from_nanos(nanos)),
  };
  ensure!(input.0.is_empty(), "Software record has trailing data");
  let software = Software {
    power: power::snapshot::Snapshot { cores, executable },
    debug,
  };
  validate(&software, &actions)?;
  Ok(State { software, actions })
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
  fn take(&mut self, count: usize) -> anyhow::Result<&'a [u8]> {
    ensure!(count <= self.0.len(), "Software record is truncated");
    let (value, rest) = self.0.split_at(count);
    self.0 = rest;
    Ok(value)
  }

  fn byte(&mut self) -> anyhow::Result<u8> {
    Ok(self.take(1)?[0])
  }
  fn number(&mut self) -> anyhow::Result<u64> {
    Ok(u64::from_le_bytes(self.take(8)?.try_into()?))
  }
  fn boolean(&mut self) -> anyhow::Result<bool> {
    let value = self.byte()?;
    ensure!(value <= 1, "Software record boolean is invalid");
    Ok(value == 1)
  }
}
