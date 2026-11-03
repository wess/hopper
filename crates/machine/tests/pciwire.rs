use machine::devices::{
  pci::{self, Function},
  virtio::pci::msix,
};
use sha2::{Digest, Sha256};

fn function() -> anyhow::Result<Function> {
  let mut function = Function::new(0x1af4, 0x1052, 0x098000, 1)?;
  function.add_bar(0, 0x4000, true)?;
  function.add_bar(2, 0x1000, false)?;
  function.add_msix(3, 2, 0, 0x800)?;
  Ok(function)
}

fn checksum(bytes: &mut [u8]) {
  let end = bytes.len() - 32;
  let checksum = Sha256::digest(&bytes[..end]);
  bytes[end..].copy_from_slice(&checksum);
}

#[test]
fn pci_configuration_bar_addresses_and_probe_halves_reopen_against_fresh_layout(
) -> anyhow::Result<()> {
  let mut saved = function()?;
  saved.write(4, 2, 6)?;
  saved.write(0x10, 4, 0x20000004)?;
  saved.write(0x14, 4, 1)?;
  saved.write(0x14, 4, u32::MAX)?;
  saved.write(0x18, 4, u32::MAX)?;
  saved.interrupt_status(true);
  let bytes = pci::wire::encode(&saved)?;
  drop(saved);
  let reopened = pci::wire::decode(&bytes, &function()?)?;
  assert_eq!(reopened.memory(0x120000000), Some((0, 0)));
  assert_eq!(reopened.read(0x14, 4)?, u32::MAX);
  assert_eq!(reopened.read(0x18, 4)?, 0xfffff000);
  assert!(reopened.bus_master());
  assert_eq!(reopened.read(6, 1)? & 8, 8);
  assert_eq!(pci::wire::encode(&reopened)?, bytes);
  Ok(())
}

#[test]
fn pci_corruption_foreign_layout_and_rechecksummed_reserved_fields_reject() -> anyhow::Result<()> {
  let expected = function()?;
  let bytes = pci::wire::encode(&expected)?;
  for end in 0..bytes.len() {
    assert!(pci::wire::decode(&bytes[..end], &expected).is_err());
  }
  for offset in 0..bytes.len() {
    let mut changed = bytes.clone();
    changed[offset] ^= 1;
    assert!(pci::wire::decode(&changed, &expected).is_err());
  }
  for offset in [0, 4, 10, 14, 4106, 4107, 4108, 4116, 4117, 4125] {
    let mut changed = bytes.clone();
    changed[offset] ^= 8;
    checksum(&mut changed);
    assert!(
      pci::wire::decode(&changed, &expected).is_err(),
      "Accepted unsafe PCI field at {offset}"
    );
  }
  let foreign = Function::new(0x1af4, 0x1050, 0x038000, 1)?;
  assert!(pci::wire::decode(&bytes, &foreign).is_err());
  Ok(())
}

#[test]
fn masked_msix_pending_message_survives_reopening_and_delivers_once() -> anyhow::Result<()> {
  let mut state = msix::create(64)?;
  msix::write(&mut state, 63 * 16, 8, 0x30000040)?;
  msix::write(&mut state, 63 * 16 + 8, 4, 64)?;
  msix::raise(&mut state, 63);
  let bytes = msix::wire::encode(&state)?;
  drop(state);
  let mut state = msix::wire::decode(&bytes)?;
  assert!(msix::take(&mut state, true, false).is_empty());
  msix::write(&mut state, 63 * 16 + 12, 4, 0)?;
  assert_eq!(
    msix::take(&mut state, true, false),
    vec![msix::Message {
      address: 0x30000040,
      data: 64
    }]
  );
  assert!(msix::take(&mut state, true, false).is_empty());
  for end in 0..bytes.len() {
    assert!(msix::wire::decode(&bytes[..end]).is_err());
  }
  for offset in 0..bytes.len() {
    let mut changed = bytes.clone();
    changed[offset] ^= 1;
    assert!(msix::wire::decode(&changed).is_err());
  }
  let mut invalid = msix::wire::encode(&msix::create(1)?)?;
  invalid[17] = 2;
  checksum(&mut invalid);
  assert!(msix::wire::decode(&invalid).is_err());
  let mut invalid = msix::wire::encode(&msix::create(1)?)?;
  invalid[21] = 2;
  checksum(&mut invalid);
  assert!(msix::wire::decode(&invalid).is_err());
  Ok(())
}
