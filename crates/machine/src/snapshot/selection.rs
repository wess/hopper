use anyhow::ensure;
use sha2::{Digest, Sha256};

pub const SIZE: usize = 149;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
  Prepared,
  Attempted,
  Active,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Record {
  pub machine: [u8; 16],
  pub generation: [u8; 16],
  pub snapshot: [u8; 16],
  pub configuration: [u8; 32],
  pub rollback: [u8; 16],
  pub previous: Option<[u8; 16]>,
  pub phase: Phase,
}

impl Record {
  pub fn encode(&self) -> anyhow::Result<Vec<u8>> {
    ensure!(
      self.machine != [0; 16]
        && self.generation != [0; 16]
        && self.snapshot != [0; 16]
        && self.configuration != [0; 32]
        && self.rollback != [0; 16]
        && self.rollback != self.snapshot
        && self
          .previous
          .is_none_or(|id| id != [0; 16] && id != self.generation),
      "Invalid native restore selection"
    );
    let mut bytes = Vec::from(b"HRR1");
    for value in [self.machine, self.generation, self.snapshot] {
      bytes.extend_from_slice(&value);
    }
    bytes.extend_from_slice(&self.configuration);
    bytes.extend_from_slice(&self.rollback);
    bytes.extend_from_slice(&self.previous.unwrap_or([0; 16]));
    bytes.push(match self.phase {
      Phase::Prepared => 1,
      Phase::Attempted => 2,
      Phase::Active => 3,
    });
    bytes.extend_from_slice(&Sha256::digest(&bytes));
    Ok(bytes)
  }

  pub fn decode(bytes: &[u8], machine: [u8; 16]) -> anyhow::Result<Self> {
    ensure!(
      bytes.len() == SIZE && &bytes[..4] == b"HRR1",
      "Invalid native restore record size/version"
    );
    ensure!(
      Sha256::digest(&bytes[..117])[..] == bytes[117..],
      "Native restore record checksum mismatch"
    );
    let previous: [u8; 16] = bytes[100..116].try_into()?;
    let record = Self {
      machine: bytes[4..20].try_into()?,
      generation: bytes[20..36].try_into()?,
      snapshot: bytes[36..52].try_into()?,
      configuration: bytes[52..84].try_into()?,
      rollback: bytes[84..100].try_into()?,
      previous: (previous != [0; 16]).then_some(previous),
      phase: match bytes[116] {
        1 => Phase::Prepared,
        2 => Phase::Attempted,
        3 => Phase::Active,
        _ => anyhow::bail!("Unknown restore phase"),
      },
    };
    ensure!(
      record.machine == machine,
      "Native restore record belongs to another VM"
    );
    record.encode()?;
    Ok(record)
  }
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub fn read_at(root: &std::fs::File, machine: [u8; 16]) -> anyhow::Result<Record> {
  use std::io::Read;
  super::package::fs::private(root)?;
  let file = super::package::fs::open(root, c"restore.hrr", SIZE as u64)?;
  let mut bytes = Vec::new();
  file.take(SIZE as u64 + 1).read_to_end(&mut bytes)?;
  Record::decode(&bytes, machine)
}
