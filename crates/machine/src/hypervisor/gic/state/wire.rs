//! HGI1 preserves the SDK's opaque versioned data; SDK compatibility is checked at restore.

use super::{State, LIMIT};
use anyhow::ensure;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

#[cfg(test)]
#[path = "../../../../tests/gic/codec.rs"]
mod tests;

pub const MAX: usize = LIMIT + 40;

pub fn encode(state: &State) -> Zeroizing<Vec<u8>> {
  let mut bytes = Zeroizing::new(Vec::with_capacity(state.bytes().len() + 40));
  bytes.extend_from_slice(b"HGI1");
  bytes.extend_from_slice(&(state.bytes().len() as u32).to_le_bytes());
  bytes.extend_from_slice(state.bytes());
  let checksum = Sha256::digest(&*bytes);
  bytes.extend_from_slice(&checksum);
  bytes
}

pub fn decode(bytes: &[u8]) -> anyhow::Result<State> {
  ensure!(
    (41..=MAX).contains(&bytes.len()),
    "Interrupt record size is invalid"
  );
  ensure!(
    &bytes[..4] == b"HGI1",
    "Interrupt record format is unsupported"
  );
  let end = bytes.len() - 32;
  ensure!(
    Sha256::digest(&bytes[..end])[..] == bytes[end..],
    "Interrupt record checksum is invalid"
  );
  let size = u32::from_le_bytes(bytes[4..8].try_into()?) as usize;
  ensure!(size == end - 8, "Interrupt record length is invalid");
  State::from_bytes(&bytes[8..end])
}
