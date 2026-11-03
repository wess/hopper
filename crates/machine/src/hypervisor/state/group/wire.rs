//! HCP1 bundles architectural registers, cycle-only PMU and frozen clock; not VM state.

use super::{clock, Captured};
use crate::hypervisor::{pmu, state};
use anyhow::ensure;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

#[cfg(test)]
#[path = "../../../../tests/cpu/combined.rs"]
mod tests;

pub const MAX: usize = 8192;

fn validate(saved: &Captured, rate: (u32, u32)) -> anyhow::Result<()> {
  let (affinity, features, frequency) = saved.pmu.binding();
  ensure!(
    saved.cpu.features[5] == features && saved.cpu.system.first() == Some(&affinity),
    "CPU checkpoint PMU binding is incompatible"
  );
  ensure!(
    u128::from(frequency) * u128::from(rate.0) == 1_000_000_000u128 * u128::from(rate.1),
    "CPU checkpoint counter frequency is incompatible with host ticks"
  );
  Ok(())
}

pub fn encode(saved: &Captured) -> anyhow::Result<Zeroizing<Vec<u8>>> {
  let rate = clock::rate()?;
  validate(saved, rate)?;
  let cpu = state::wire::encode(&saved.cpu)?;
  let pmu = pmu::wire::encode(&saved.pmu)?;
  let mut bytes = Zeroizing::new(Vec::with_capacity(28 + cpu.len() + pmu.len() + 32));
  bytes.extend_from_slice(b"HCP1");
  bytes.extend_from_slice(&rate.0.to_le_bytes());
  bytes.extend_from_slice(&rate.1.to_le_bytes());
  bytes.extend_from_slice(&saved.clock.counter().to_le_bytes());
  bytes.extend_from_slice(&(cpu.len() as u32).to_le_bytes());
  bytes.extend_from_slice(&(pmu.len() as u32).to_le_bytes());
  bytes.extend_from_slice(&cpu);
  bytes.extend_from_slice(&pmu);
  let checksum = Sha256::digest(&*bytes);
  bytes.extend_from_slice(&checksum);
  ensure!(bytes.len() <= MAX, "CPU checkpoint exceeds its bound");
  Ok(bytes)
}

pub fn decode(bytes: &[u8]) -> anyhow::Result<Captured> {
  ensure!(
    (60..=MAX).contains(&bytes.len()),
    "CPU checkpoint size is invalid"
  );
  ensure!(
    &bytes[..4] == b"HCP1",
    "CPU checkpoint format is unsupported"
  );
  let end = bytes.len() - 32;
  ensure!(
    Sha256::digest(&bytes[..end])[..] == bytes[end..],
    "CPU checkpoint checksum is invalid"
  );
  let rate = (
    u32::from_le_bytes(bytes[4..8].try_into()?),
    u32::from_le_bytes(bytes[8..12].try_into()?),
  );
  ensure!(
    rate == clock::rate()?,
    "CPU checkpoint host timer units are incompatible"
  );
  let counter = u64::from_le_bytes(bytes[12..20].try_into()?);
  let cpu = u32::from_le_bytes(bytes[20..24].try_into()?) as usize;
  let pmu = u32::from_le_bytes(bytes[24..28].try_into()?) as usize;
  ensure!(
    cpu <= state::wire::MAX && pmu == pmu::wire::SIZE && 28 + cpu + pmu == end,
    "CPU checkpoint component lengths are invalid"
  );
  let saved = Captured {
    cpu: state::wire::decode(&bytes[28..28 + cpu])?,
    pmu: pmu::wire::decode(&bytes[28 + cpu..end])?,
    clock: clock::Frozen::freeze(counter, 0),
  };
  validate(&saved, rate)?;
  Ok(saved)
}
