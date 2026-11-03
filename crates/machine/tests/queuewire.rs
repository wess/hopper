mod support;
use machine::devices::virtio::queue::{self, snapshot::wire};
use sha2::{Digest, Sha256};
use support::{Ram, BASE};

#[test]
fn queue_records_reject_corruption_bad_geometry_and_uninitialized_progress() -> anyhow::Result<()> {
  let mut ram = Ram::default();
  support::descriptor(&mut ram, 0, BASE + 0x1000, 8, 2, 0);
  let queue = queue::create(&ram, 8, BASE, BASE + 0x100, BASE + 0x200)?;
  let bytes = wire::encode(&queue::snapshot::capture(&queue)?)?;
  for end in 0..bytes.len() {
    assert!(wire::decode(&bytes[..end]).is_err());
  }
  for offset in 0..bytes.len() {
    let mut changed = bytes.clone();
    changed[offset] ^= 1;
    assert!(wire::decode(&changed).is_err());
  }
  for (offset, value) in [(0, b'X'), (4, 0), (4, 3), (6, 1), (30, 1), (32, 1), (34, 2)] {
    let mut changed = bytes.clone();
    changed[offset] = value;
    checksum(&mut changed);
    assert!(wire::decode(&changed).is_err());
  }
  for (offset, value) in [(14, BASE), (22, u64::MAX - 3)] {
    let mut changed = bytes.clone();
    changed[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    checksum(&mut changed);
    assert!(wire::decode(&changed).is_err());
  }
  Ok(())
}

fn checksum(bytes: &mut [u8]) {
  let checksum = Sha256::digest(&bytes[..35]);
  bytes[35..].copy_from_slice(&checksum);
}
