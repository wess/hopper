use super::Clock;
use anyhow::ensure;
use sha2::{Digest, Sha256};

pub const SIZE: usize = 61;

pub fn encode(clock: &Clock) -> [u8; SIZE] {
  let mut bytes = [0; SIZE];
  bytes[..4].copy_from_slice(b"HRT1");
  bytes[4..12].copy_from_slice(&clock.offset.to_le_bytes());
  for (slot, value) in [clock.halted, clock.loaded, clock.matched, clock.last]
    .iter()
    .enumerate()
  {
    bytes[12 + slot * 4..16 + slot * 4].copy_from_slice(&value.to_le_bytes());
  }
  bytes[28] = clock.enabled as u8 | (clock.mask as u8) << 1 | (clock.pending as u8) << 2;
  let checksum = Sha256::digest(&bytes[..29]);
  bytes[29..].copy_from_slice(&checksum);
  bytes
}

pub fn decode(bytes: &[u8]) -> anyhow::Result<Clock> {
  ensure!(
    bytes.len() == SIZE && &bytes[..4] == b"HRT1",
    "Unknown or malformed RTC record"
  );
  ensure!(
    Sha256::digest(&bytes[..29])[..] == bytes[29..],
    "RTC record checksum is invalid"
  );
  let offset = i64::from_le_bytes(bytes[4..12].try_into()?);
  ensure!(
    (-i64::from(u32::MAX)..=i64::from(u32::MAX)).contains(&offset) && bytes[28] & !7 == 0,
    "RTC record fields are invalid"
  );
  ensure!(
    bytes[28] & 1 != 0 || bytes[12..16] == bytes[24..28],
    "Stopped RTC record has inconsistent counter state"
  );
  Ok(Clock {
    offset,
    halted: u32::from_le_bytes(bytes[12..16].try_into()?),
    loaded: u32::from_le_bytes(bytes[16..20].try_into()?),
    matched: u32::from_le_bytes(bytes[20..24].try_into()?),
    last: u32::from_le_bytes(bytes[24..28].try_into()?),
    enabled: bytes[28] & 1 != 0,
    mask: bytes[28] & 2 != 0,
    pending: bytes[28] & 4 != 0,
  })
}
