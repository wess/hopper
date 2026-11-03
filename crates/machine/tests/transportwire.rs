use machine::devices::virtio::{
  input::{self, Kind},
  pci::{self, snapshot},
};
use sha2::{Digest, Sha256};

fn saved() -> anyhow::Result<snapshot::State> {
  snapshot::capture(&pci::controller(input::create(Kind::Keyboard))?)
}

#[test]
fn transport_records_reject_damage_truncation_foreign_layout_and_extensions() -> anyhow::Result<()>
{
  let bytes = snapshot::wire::encode(&saved()?)?;
  let target = pci::controller(input::create(Kind::Keyboard))?;
  let decoded = snapshot::wire::decode(&bytes, &target)?;
  assert_eq!(snapshot::wire::encode(&decoded)?, bytes);
  let reopened = snapshot::wire::reopen(&bytes, saved()?)?;
  assert_eq!(snapshot::wire::encode(&reopened)?, bytes);
  for end in 0..bytes.len() {
    assert!(snapshot::wire::reopen(&bytes[..end], saved()?).is_err());
  }
  for offset in 0..bytes.len() {
    let mut changed = bytes.clone();
    changed[offset] ^= 1;
    assert!(snapshot::wire::reopen(&changed, saved()?).is_err());
  }
  for offset in [0, 4, 5, 13, 15, 16] {
    let mut changed = bytes.clone();
    changed[offset] ^= 8;
    let end = changed.len() - 32;
    let checksum = Sha256::digest(&changed[..end]);
    changed[end..].copy_from_slice(&checksum);
    assert!(snapshot::wire::reopen(&changed, saved()?).is_err());
  }
  let mut extended = bytes.to_vec();
  extended.insert(extended.len() - 32, 0);
  let end = extended.len() - 32;
  let checksum = Sha256::digest(&extended[..end]);
  extended[end..].copy_from_slice(&checksum);
  assert!(snapshot::wire::reopen(&extended, saved()?).is_err());
  assert!(snapshot::wire::reopen(&vec![0; snapshot::wire::MAX + 1], saved()?).is_err());
  Ok(())
}
