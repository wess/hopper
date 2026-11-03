use machine::devices::flash::{
  self,
  snapshot::{self, wire},
};
use sha2::{Digest, Sha256};

fn packet() -> anyhow::Result<zeroize::Zeroizing<Vec<u8>>> {
  let mut bank = flash::create(vec![0xff; 0x10000], 0x1000)?;
  for (address, value) in [(128, 0xe8), (128, 2), (128, 0x12345678), (132, 0xabcdef01)] {
    flash::write(&mut bank, address, value)?;
  }
  wire::encode(&snapshot::capture(&bank))
}

fn checksum(bytes: &mut [u8]) {
  let end = bytes.len() - 32;
  let checksum = Sha256::digest(&bytes[..end]);
  bytes[end..].copy_from_slice(&checksum);
}

#[test]
fn damaged_truncated_and_extended_records_reject() -> anyhow::Result<()> {
  let bytes = packet()?;
  assert_eq!(wire::encode(&wire::decode(&bytes)?)?, bytes);
  for end in 0..bytes.len() {
    assert!(wire::decode(&bytes[..end]).is_err());
  }
  for offset in 0..bytes.len() {
    let mut changed = bytes.clone();
    changed[offset] ^= 1;
    assert!(wire::decode(&changed).is_err());
  }
  let mut extended = bytes.to_vec();
  extended.insert(extended.len() - 32, 0);
  checksum(&mut extended);
  assert!(wire::decode(&extended).is_err());
  assert!(wire::decode(&vec![0; wire::MAX + 1]).is_err());
  Ok(())
}

#[test]
fn rechecksummed_unsafe_pending_commands_and_layouts_reject() -> anyhow::Result<()> {
  let bytes = packet()?;
  for (offset, value) in [
    (0, b'X'),
    (16, 2),
    (32, 4),
    (32, 0),
    (33, 0),
    (34, 7),
    (39, 0),
    (39, 31),
    (40, 33),
  ] {
    let mut changed = bytes.clone();
    changed[offset] = value;
    checksum(&mut changed);
    assert!(
      wire::decode(&changed).is_err(),
      "Accepted invalid field at {offset}"
    );
  }
  for (offset, value) in [
    (4, 0u32),
    (4, 0x8000001),
    (8, 0),
    (8, 511),
    (12, u32::MAX),
    (35, 0x10000),
    (41, 129),
    (49, 128),
    (49, 256),
    (49, u32::MAX),
  ] {
    let mut changed = bytes.clone();
    changed[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    checksum(&mut changed);
    assert!(
      wire::decode(&changed).is_err(),
      "Accepted invalid address/dimension at {offset}"
    );
  }
  for command in [0x20, 0x60] {
    let mut bank = flash::create(vec![0xff; 0x10000], 0x1000)?;
    flash::write(&mut bank, 0x1000, command)?;
    let mut changed = wire::encode(&snapshot::capture(&bank))?;
    changed[35..39].copy_from_slice(&0x1004u32.to_le_bytes());
    checksum(&mut changed);
    assert!(wire::decode(&changed).is_err());
  }
  Ok(())
}

#[test]
fn largest_protection_inventory_and_full_write_buffer_reopen() -> anyhow::Result<()> {
  let bank = flash::create(vec![0xff; 0x8000000], 512)?;
  let bytes = wire::encode(&snapshot::capture(&bank))?;
  drop(bank);
  assert!(bytes.len() <= wire::MAX);
  assert_eq!(wire::encode(&wire::decode(&bytes)?)?, bytes);
  let mut bank = flash::create(vec![0xff; 0x10000], 0x1000)?;
  flash::write(&mut bank, 0, 0xe8)?;
  flash::write(&mut bank, 0, 31)?;
  for address in (0..128).step_by(4) {
    flash::write(&mut bank, address, address as u32)?;
  }
  let saved = wire::decode(&wire::encode(&snapshot::capture(&bank))?)?;
  let mut restored = snapshot::restore(&saved, vec![0xff; 0x10000])?;
  assert_eq!(flash::write(&mut restored, 0, 0xd0)?, Some(0..128));
  flash::write(&mut restored, 0, 0xff)?;
  assert_eq!(flash::read(&restored, 124, 4)?, 124);
  Ok(())
}
