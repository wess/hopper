use machine::devices::serial::{self, Console};

#[test]
fn transmit_requires_enabled_uart_and_transmitter() {
  let mut console = Console::default();
  assert_eq!(serial::write(&mut console, 0, b'A'.into()), None);
  serial::write(&mut console, 0x30, 0x101);
  assert_eq!(serial::write(&mut console, 0, b'A'.into()), Some(b'A'));
  assert_eq!(serial::read(&mut console, 0x18), 0x90);
}

#[test]
fn receiving_preserves_order_and_bounds_the_fifo() {
  let mut console = Console::default();
  assert!(!serial::receive(&mut console, b'A'));
  serial::write(&mut console, 0x30, 0x301);
  serial::write(&mut console, 0x2c, 0x10);
  for byte in 0..16 {
    assert!(serial::receive(&mut console, byte));
  }
  assert!(!serial::receive(&mut console, 16));
  assert_eq!(serial::read(&mut console, 0x18), 0xc0);
  for byte in 0..16 {
    assert_eq!(serial::read(&mut console, 0), byte);
  }
  assert_eq!(serial::read(&mut console, 0x18), 0x90);
}

#[test]
fn baud_and_line_registers_mask_reserved_bits() {
  let mut console = Console::default();
  for (offset, mask) in [(0x24, 0xffff), (0x28, 0x3f), (0x2c, 0xff)] {
    serial::write(&mut console, offset, u32::MAX);
    assert_eq!(serial::read(&mut console, offset), mask);
  }
}

#[test]
fn fifo_disabled_keeps_only_one_received_character() {
  let mut console = Console::default();
  serial::write(&mut console, 0x30, 0x301);
  assert!(serial::receive(&mut console, b'A'));
  assert!(!serial::receive(&mut console, b'B'));
  assert_eq!(serial::read(&mut console, 0), b'A'.into());
}
