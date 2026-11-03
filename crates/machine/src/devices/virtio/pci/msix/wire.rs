//! HMX1 preserves the bounded MSI-X table and pending bits.

use super::State;
use anyhow::ensure;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

pub const MAX: usize = 1069;

fn validate(state: &State) -> anyhow::Result<()> {
  let count = state.entries.len();
  ensure!(
    (1..=64).contains(&count),
    "MSI-X record table size is invalid"
  );
  ensure!(
    state.entries.iter().all(|entry| entry[3] <= 1) && (count == 64 || state.pending >> count == 0),
    "MSI-X record reserved bits are invalid"
  );
  Ok(())
}

pub fn encode(state: &State) -> anyhow::Result<Zeroizing<Vec<u8>>> {
  validate(state)?;
  let mut bytes = Zeroizing::new(Vec::from(b"HMX1"));
  bytes.push(state.entries.len() as u8);
  for entry in &state.entries {
    for word in entry {
      bytes.extend_from_slice(&word.to_le_bytes());
    }
  }
  bytes.extend_from_slice(&state.pending.to_le_bytes());
  let checksum = Sha256::digest(&*bytes);
  bytes.extend_from_slice(&checksum);
  Ok(bytes)
}

pub fn decode(bytes: &[u8]) -> anyhow::Result<State> {
  ensure!(
    (61..=MAX).contains(&bytes.len()),
    "MSI-X record size is invalid"
  );
  let end = bytes.len() - 32;
  ensure!(
    Sha256::digest(&bytes[..end])[..] == bytes[end..],
    "MSI-X record checksum is invalid"
  );
  ensure!(&bytes[..4] == b"HMX1", "MSI-X record format is unsupported");
  let count = bytes[4] as usize;
  ensure!(
    (1..=64).contains(&count) && 5 + count * 16 + 8 == end,
    "MSI-X record table size is invalid"
  );
  let mut entries = Vec::with_capacity(count);
  for index in 0..count {
    let mut entry = [0; 4];
    for (word, value) in entry.iter_mut().enumerate() {
      let offset = 5 + index * 16 + word * 4;
      *value = u32::from_le_bytes(bytes[offset..offset + 4].try_into()?);
    }
    entries.push(entry);
  }
  let state = State {
    entries,
    pending: u64::from_le_bytes(bytes[end - 8..end].try_into()?),
  };
  validate(&state)?;
  Ok(state)
}
