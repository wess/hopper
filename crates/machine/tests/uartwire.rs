use machine::devices::serial::{self, wire};
use sha2::{Digest, Sha256};

fn packet() -> anyhow::Result<zeroize::Zeroizing<Vec<u8>>> {
  let mut console = serial::Console::default();
  for (offset, value) in [(0x30, 0x301), (0x2c, 0x10), (0x24, 0x1234), (0x28, 0x21)] {
    serial::write(&mut console, offset, value);
  }
  for byte in 0..16 {
    assert!(serial::receive(&mut console, byte));
  }
  // changing FIFO mode does not discard bytes already queued by the device.
  serial::write(&mut console, 0x2c, 0);
  wire::encode(&console)
}

#[test]
fn uart_registers_pending_input_and_fifo_mode_transition_survive_reopening() -> anyhow::Result<()> {
  let bytes = packet()?;
  for _ in 0..2 {
    let mut console = wire::decode(&bytes)?;
    for (offset, expected) in [(0x30, 0x301), (0x2c, 0), (0x24, 0x1234), (0x28, 0x21)] {
      assert_eq!(serial::read(&mut console, offset), expected);
    }
    for byte in 0..16 {
      assert_eq!(serial::read(&mut console, 0), byte);
    }
    assert_eq!(serial::read(&mut console, 0x18) & 0x10, 0x10);
    assert_eq!(serial::write(&mut console, 0, b'B'.into()), Some(b'B'));
    assert!(serial::receive(&mut console, 42));
    assert!(!serial::receive(&mut console, 43));
  }
  assert_eq!(wire::encode(&wire::decode(&bytes)?)?, bytes);
  Ok(())
}

#[test]
fn malformed_uart_records_reject() -> anyhow::Result<()> {
  let bytes = packet()?;
  for end in 0..bytes.len() {
    assert!(wire::decode(&bytes[..end]).is_err());
  }
  for offset in 0..bytes.len() {
    let mut changed = bytes.clone();
    changed[offset] ^= 1;
    assert!(wire::decode(&changed).is_err());
  }
  for (offset, value) in [(0, b'X'), (4, 8), (9, 1), (14, 1), (16, 64), (20, 17)] {
    let mut changed = bytes.clone();
    changed[offset] = value;
    let end = changed.len() - 32;
    let checksum = Sha256::digest(&changed[..end]);
    changed[end..].copy_from_slice(&checksum);
    assert!(wire::decode(&changed).is_err());
  }
  let mut oversized = wire::decode(&bytes)?;
  oversized.control = u32::MAX;
  assert!(wire::encode(&oversized).is_err());
  Ok(())
}
