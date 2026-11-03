use super::{decode, encode, Captured, MAX};
use crate::hypervisor::{pmu, state::clock};
use sha2::{Digest, Sha256};

use crate::hypervisor::state::fixture;

fn checksum(bytes: &mut [u8]) {
  let end = bytes.len() - 32;
  let digest = Sha256::digest(&bytes[..end]);
  bytes[end..].copy_from_slice(&digest);
}

fn packet() -> anyhow::Result<Captured> {
  let cpu = fixture::state();
  let (numerator, denominator) = clock::rate()?;
  let frequency = 1_000_000_000u64 * u64::from(denominator) / u64::from(numerator);
  let mut bytes = Vec::from(b"HPM1");
  for value in [cpu.system[0], cpu.features[5], frequency] {
    bytes.extend_from_slice(&value.to_le_bytes());
  }
  bytes.extend_from_slice(&8u16.to_le_bytes());
  for (register, value) in [
    (0xdce0u16, 0x41u64),
    (0xdce1, 1 << 31),
    (0xdce3, 1 << 31),
    (0xdce5, 7),
    (0xdce8, 0x1234567800000000),
    (0xdcf0, 15),
    (0xc4f1, 1 << 31),
    (0xdf7f, 0xc8000000),
  ] {
    bytes.extend_from_slice(&register.to_le_bytes());
    bytes.extend_from_slice(&value.to_le_bytes());
  }
  let digest = Sha256::digest(&bytes);
  bytes.extend_from_slice(&digest);
  Ok(Captured {
    cpu,
    pmu: pmu::wire::decode(&bytes)?,
    clock: clock::Frozen::freeze(12345, 56),
  })
}

#[test]
fn combined_registers_pmu_and_frozen_epoch_round_trip() -> anyhow::Result<()> {
  let saved = packet()?;
  let encoded = encode(&saved)?;
  let reopened = decode(&encoded)?;
  assert!(saved.cpu == reopened.cpu);
  assert!(saved.clock == reopened.clock);
  assert!(pmu::wire::encode(&saved.pmu)? == pmu::wire::encode(&reopened.pmu)?);
  assert!(encode(&reopened)? == encoded);
  Ok(())
}

#[test]
fn combined_and_pmu_records_reject_truncation_damage_and_extensions() -> anyhow::Result<()> {
  let saved = packet()?;
  let encoded = encode(&saved)?;
  for end in 0..encoded.len() {
    assert!(decode(&encoded[..end]).is_err());
  }
  for index in 0..encoded.len() {
    let mut changed = encoded.clone();
    changed[index] ^= 1;
    assert!(decode(&changed).is_err());
  }
  assert!(decode(&vec![0; MAX + 1]).is_err());
  let encoded = pmu::wire::encode(&saved.pmu)?;
  for end in 0..encoded.len() {
    assert!(pmu::wire::decode(&encoded[..end]).is_err());
  }
  for index in 0..encoded.len() {
    let mut changed = encoded.clone();
    changed[index] ^= 1;
    assert!(pmu::wire::decode(&changed).is_err());
  }
  let mut extended = encoded.clone();
  extended.push(0);
  assert!(pmu::wire::decode(&extended).is_err());
  Ok(())
}

#[test]
fn rechecksummed_foreign_rates_affinity_features_and_registers_reject() -> anyhow::Result<()> {
  let encoded = encode(&packet()?)?;
  let core = u32::from_le_bytes(encoded[20..24].try_into()?) as usize;
  let pmu = 28 + core;
  for offset in [4, 8, 20, 24] {
    let mut changed = encoded.clone();
    changed[offset] ^= 1;
    checksum(&mut changed);
    assert!(decode(&changed).is_err());
  }
  for offset in [4, 12, 20, 28, 30] {
    let mut changed = encoded.clone();
    changed[pmu + offset] ^= 1;
    checksum(&mut changed[pmu..pmu + pmu::wire::SIZE]);
    checksum(&mut changed);
    assert!(decode(&changed).is_err(), "Accepted mixed CPU/PMU metadata");
  }
  let mut events = encoded.clone();
  events[pmu + 33] |= 8;
  checksum(&mut events[pmu..pmu + pmu::wire::SIZE]);
  checksum(&mut events);
  assert!(decode(&events).is_err());
  Ok(())
}

#[test]
fn encoding_mixed_cpu_affinity_and_pmu_rejects() -> anyhow::Result<()> {
  let mut saved = packet()?;
  saved.cpu.system[0] ^= 1;
  assert!(encode(&saved).is_err());
  let mut saved = packet()?;
  saved.cpu.features[5] ^= 1;
  assert!(encode(&saved).is_err());
  Ok(())
}
