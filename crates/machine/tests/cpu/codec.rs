use super::{decode, encode, MAX};
use sha2::{Digest, Sha256};

use crate::hypervisor::state::fixture;

fn checksum(bytes: &mut [u8]) {
  let end = bytes.len() - 32;
  let digest = Sha256::digest(&bytes[..end]);
  bytes[end..].copy_from_slice(&digest);
}

#[test]
fn all_register_banks_and_maximum_debug_inventory_round_trip() -> anyhow::Result<()> {
  let saved = fixture::state();
  let encoded = encode(&saved)?;
  assert!(encoded.len() <= MAX);
  assert!(decode(&encoded)? == saved);
  let reopened = decode(&encoded)?;
  assert!(encode(&reopened)? == encoded);
  Ok(())
}

#[test]
fn every_truncation_bit_damage_and_size_extension_rejects() -> anyhow::Result<()> {
  let encoded = encode(&fixture::state())?;
  for end in 0..encoded.len() {
    assert!(decode(&encoded[..end]).is_err());
  }
  for index in 0..encoded.len() {
    let mut damaged = encoded.clone();
    damaged[index] ^= 1;
    assert!(decode(&damaged).is_err());
  }
  let mut extended = encoded.clone();
  extended.push(0);
  assert!(decode(&extended).is_err());
  assert!(decode(&vec![0; MAX + 1]).is_err());
  Ok(())
}

#[test]
fn rechecksummed_foreign_registers_dimensions_flags_and_vectors_reject() -> anyhow::Result<()> {
  let encoded = encode(&fixture::state())?;
  // fixed HCS1 offsets: features, general, SIMD, system and interrupt banks.
  for offset in [8, 10, 130, 132, 552, 554, 1194, 1196, 1586, 1588] {
    let mut altered = encoded.clone();
    altered[offset] ^= 1;
    checksum(&mut altered);
    assert!(
      decode(&altered).is_err(),
      "Accepted modified schema at {offset}"
    );
  }
  let mut flags = encoded.clone();
  let offset = flags.len() - 33;
  flags[offset] |= 4;
  checksum(&mut flags);
  assert!(decode(&flags).is_err());
  for (offset, bit) in [(22 + 4, 1), (32 + 3, 1)] {
    let mut wider = encoded.clone();
    wider[offset] |= bit;
    checksum(&mut wider);
    assert!(
      decode(&wider).is_err(),
      "Accepted unsupported wider vector state"
    );
  }
  Ok(())
}

#[test]
fn debug_register_identity_and_trailing_payload_reject() -> anyhow::Result<()> {
  let encoded = encode(&fixture::state())?;
  let mut foreign = encoded.clone();
  // the nine interrupt entries precede the debug-bank count and identities.
  let debug = 1586 + 2 + 9 * 10;
  foreign[debug + 2..debug + 4].copy_from_slice(&0xc080u16.to_le_bytes());
  checksum(&mut foreign);
  assert!(decode(&foreign).is_err());
  let mut trailing = encoded.clone();
  let end = trailing.len() - 32;
  trailing.insert(end, 0);
  let length = (trailing.len() - 40) as u32;
  trailing[4..8].copy_from_slice(&length.to_le_bytes());
  checksum(&mut trailing);
  assert!(decode(&trailing).is_err());
  Ok(())
}

#[test]
fn encoding_incomplete_or_wider_state_rejects() {
  let mut missing = fixture::state();
  missing.system.pop();
  assert!(encode(&missing).is_err());
  let mut missing = fixture::state();
  missing.interrupts.pop();
  assert!(encode(&missing).is_err());
  let mut wider = fixture::state();
  wider.features[1] |= 1 << 32;
  assert!(encode(&wider).is_err());
}
