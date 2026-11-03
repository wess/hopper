//! HPM1 records bind the cycle-only bank to its CPU affinity/features and timer units.

use super::{State, SAVED};
use anyhow::ensure;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

pub const SIZE: usize = 142;

pub fn encode(state: &State) -> anyhow::Result<Zeroizing<Vec<u8>>> {
  state.validate()?;
  let mut bytes = Zeroizing::new(Vec::with_capacity(SIZE));
  bytes.extend_from_slice(b"HPM1");
  for value in [state.affinity, state.features, state.frequency] {
    bytes.extend_from_slice(&value.to_le_bytes());
  }
  bytes.extend_from_slice(&8u16.to_le_bytes());
  for (register, value) in SAVED.into_iter().zip(state.values) {
    bytes.extend_from_slice(&register.to_le_bytes());
    bytes.extend_from_slice(&value.to_le_bytes());
  }
  let checksum = Sha256::digest(&*bytes);
  bytes.extend_from_slice(&checksum);
  Ok(bytes)
}

pub fn decode(bytes: &[u8]) -> anyhow::Result<State> {
  ensure!(bytes.len() == SIZE, "PMU record size is invalid");
  ensure!(&bytes[..4] == b"HPM1", "PMU record format is unsupported");
  let end = SIZE - 32;
  ensure!(
    Sha256::digest(&bytes[..end])[..] == bytes[end..],
    "PMU record checksum is invalid"
  );
  let affinity = u64::from_le_bytes(bytes[4..12].try_into()?);
  let features = u64::from_le_bytes(bytes[12..20].try_into()?);
  let frequency = u64::from_le_bytes(bytes[20..28].try_into()?);
  ensure!(
    u16::from_le_bytes(bytes[28..30].try_into()?) == 8,
    "PMU bank dimensions are incompatible"
  );
  let mut values = Zeroizing::new([0; 8]);
  for (index, (register, value)) in SAVED.into_iter().zip(values.iter_mut()).enumerate() {
    let offset = 30 + index * 10;
    ensure!(
      u16::from_le_bytes(bytes[offset..offset + 2].try_into()?) == register,
      "PMU register identity is incompatible"
    );
    *value = u64::from_le_bytes(bytes[offset + 2..offset + 10].try_into()?);
  }
  let state = State {
    values: *values,
    features,
    affinity,
    frequency,
  };
  state.validate()?;
  Ok(state)
}
