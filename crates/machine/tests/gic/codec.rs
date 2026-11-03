use super::{decode, encode, State, MAX};
use sha2::{Digest, Sha256};

#[test]
fn bounded_opaque_interrupt_records_round_trip() -> anyhow::Result<()> {
  for size in [1, 257, MAX - 40] {
    let state = State::from_bytes(&vec![0xa5; size])?;
    let bytes = encode(&state);
    assert_eq!(decode(&bytes)?.bytes(), state.bytes());
    assert_eq!(bytes.len(), size + 40);
  }
  assert!(State::from_bytes(&[]).is_err());
  assert!(State::from_bytes(&vec![0; MAX - 39]).is_err());
  Ok(())
}

#[test]
fn damaged_truncated_extended_and_rechecksummed_lengths_reject() -> anyhow::Result<()> {
  let bytes = encode(&State::from_bytes(&[0xa5; 257])?);
  for end in 0..bytes.len() {
    assert!(decode(&bytes[..end]).is_err());
  }
  for index in 0..bytes.len() {
    let mut changed = bytes.clone();
    changed[index] ^= 1;
    assert!(decode(&changed).is_err());
  }
  for size in [0, 256, 258, u32::MAX] {
    let mut changed = bytes.clone();
    changed[4..8].copy_from_slice(&size.to_le_bytes());
    let end = changed.len() - 32;
    let checksum = Sha256::digest(&changed[..end]);
    changed[end..].copy_from_slice(&checksum);
    assert!(decode(&changed).is_err());
  }
  let mut extended = bytes.clone();
  extended.push(0);
  assert!(decode(&extended).is_err());
  assert!(decode(&vec![0; MAX + 1]).is_err());
  Ok(())
}
