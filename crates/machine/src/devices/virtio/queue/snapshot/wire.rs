//! HQR1 records a quiescent split ring; restored RAM is validated separately.

use super::State;
use anyhow::ensure;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

pub const SIZE: usize = 67;

fn validate(saved: &State) -> anyhow::Result<()> {
  ensure!(
    saved.size > 0 && saved.size <= 32768 && saved.size.is_power_of_two(),
    "Queue record size is invalid"
  );
  ensure!(
    saved.initialized || (saved.next_available == 0 && saved.next_used == 0),
    "Uninitialized queue has consumed entries"
  );
  let regions = [
    (saved.descriptor, 16 * u64::from(saved.size), 16),
    (saved.available, 6 + 2 * u64::from(saved.size), 2),
    (saved.used, 6 + 8 * u64::from(saved.size), 4),
  ];
  for (index, (address, length, alignment)) in regions.iter().enumerate() {
    ensure!(
      address.is_multiple_of(*alignment),
      "Queue record alignment is invalid"
    );
    let end = address
      .checked_add(*length)
      .ok_or_else(|| anyhow::anyhow!("Queue record address overflow"))?;
    for (other, length, _) in &regions[..index] {
      let other_end = other
        .checked_add(*length)
        .ok_or_else(|| anyhow::anyhow!("Queue record address overflow"))?;
      ensure!(
        end <= *other || other_end <= *address,
        "Queue record regions overlap"
      );
    }
  }
  Ok(())
}

pub fn encode(saved: &State) -> anyhow::Result<Zeroizing<Vec<u8>>> {
  validate(saved)?;
  let mut bytes = Zeroizing::new(Vec::from(b"HQR1"));
  bytes.extend_from_slice(&saved.size.to_le_bytes());
  for value in [saved.descriptor, saved.available, saved.used] {
    bytes.extend_from_slice(&value.to_le_bytes());
  }
  bytes.extend_from_slice(&saved.next_available.to_le_bytes());
  bytes.extend_from_slice(&saved.next_used.to_le_bytes());
  bytes.push(saved.initialized as u8);
  let checksum = Sha256::digest(&*bytes);
  bytes.extend_from_slice(&checksum);
  Ok(bytes)
}

pub fn decode(bytes: &[u8]) -> anyhow::Result<State> {
  ensure!(
    bytes.len() == SIZE && &bytes[..4] == b"HQR1",
    "Queue record format/size is invalid"
  );
  ensure!(
    Sha256::digest(&bytes[..35])[..] == bytes[35..],
    "Queue record checksum is invalid"
  );
  ensure!(bytes[34] <= 1, "Queue record boolean is invalid");
  let number = |offset| -> anyhow::Result<u64> {
    Ok(u64::from_le_bytes(bytes[offset..offset + 8].try_into()?))
  };
  let saved = State {
    size: u16::from_le_bytes(bytes[4..6].try_into()?),
    descriptor: number(6)?,
    available: number(14)?,
    used: number(22)?,
    next_available: u16::from_le_bytes(bytes[30..32].try_into()?),
    next_used: u16::from_le_bytes(bytes[32..34].try_into()?),
    initialized: bytes[34] == 1,
  };
  validate(&saved)?;
  Ok(saved)
}
