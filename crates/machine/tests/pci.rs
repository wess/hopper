use machine::devices::pci::{interrupt, Bus, Function, ECAM_SIZE};

fn function() -> Function {
  Function::new(0x1af4, 0x1042, 0x010000, 1).unwrap()
}

#[test]
fn legacy_interrupt_pins_swizzle_over_four_shared_lines() {
  assert_eq!(interrupt(0, 1).unwrap(), 33);
  assert_eq!(interrupt(0, 4).unwrap(), 36);
  assert_eq!(interrupt(1, 1).unwrap(), 34);
  assert_eq!(interrupt(31, 4).unwrap(), 35);
  assert!(interrupt(32, 1).is_err());
  assert!(interrupt(0, 0).is_err());
  assert!(interrupt(0, 5).is_err());
}

#[test]
fn discovery_preserves_identity_and_absent_functions() {
  let mut bus = Bus::default();
  bus.attach(31, 0, function()).unwrap();
  let base = 31 << 15;
  assert_eq!(bus.read(base, 4).unwrap(), 0x10421af4);
  bus.write(base, 4, 0).unwrap();
  assert_eq!(bus.read(base, 4).unwrap(), 0x10421af4);
  assert_eq!(bus.read(base + 8, 4).unwrap(), 0x01000001);
  assert_eq!(bus.read(base + 4096, 2).unwrap(), 0xffff);
  bus.attach(31, 7, function()).unwrap();
  assert_eq!(bus.read(base + 0x0e, 1).unwrap(), 0x80);
  assert_eq!(bus.read(ECAM_SIZE - 4096, 4).unwrap(), 0x10421af4);
  assert!(bus.read(ECAM_SIZE, 4).is_err());
  assert!(bus.read(base + 3, 2).is_err());
  assert!(bus.read(base, 8).is_err());
  assert!(bus.attach(31, 7, function()).is_err());
  assert!(bus.attach(32, 0, function()).is_err());
  assert!(bus.attach(1, 1, function()).is_err());
}

#[test]
fn firmware_can_probe_and_assign_memory_bars() {
  let mut device = function();
  device.add_bar(0, 0x4000, false).unwrap();
  device.add_bar(2, 0x1000, true).unwrap();
  assert!(device.add_bar(3, 0x1000, false).is_err());
  assert!(device.add_bar(5, 0x1000, true).is_err());
  assert!(device.add_bar(4, 0x1800, false).is_err());
  assert!(device.add_bar(4, 1 << 33, false).is_err());
  for offset in [0x10, 0x18, 0x1c] {
    device.write(offset, 4, u32::MAX).unwrap();
  }
  assert_eq!(device.read(0x10, 4).unwrap(), 0xffffc000);
  assert_eq!(device.read(0x18, 4).unwrap(), 0xfffff004);
  assert_eq!(device.read(0x1c, 4).unwrap(), u32::MAX);
  assert_eq!(device.read(0x14, 4).unwrap(), 0);
  device.write(0x10, 4, 0x20000123).unwrap();
  device.write(0x18, 4, 0x21000004).unwrap();
  device.write(0x1c, 4, 1).unwrap();
  assert_eq!(device.read(0x10, 4).unwrap(), 0x20000000);
  assert_eq!(device.read(0x18, 4).unwrap(), 0x21000004);
  assert_eq!(device.read(0x1c, 4).unwrap(), 1);
  assert_eq!(device.memory(0x20000000), None);
  device.write(4, 2, 0xffff).unwrap();
  assert_eq!(device.read(4, 2).unwrap(), 0x406);
  assert!(device.bus_master());
  assert_eq!(device.memory(0x20003fff), Some((0, 0x3fff)));
  assert_eq!(device.memory(0x20004000), None);
  assert_eq!(device.memory(0x121000800), Some((2, 0x800)));
  device.write(4, 1, 0).unwrap();
  assert!(!device.bus_master());
  assert_eq!(device.memory(0x121000800), None);
}

#[test]
fn vendor_capabilities_are_linked_and_read_only() {
  let mut device = function();
  let first = device.add_vendor_capability(&[9, 0xff, 5, 1, 2]).unwrap();
  let second = device.add_vendor_capability(&[9, 0xff, 4, 2]).unwrap();
  assert_eq!(first, 0x40);
  assert_eq!(second, 0x48);
  assert_eq!(device.read(0x34, 1).unwrap(), first as u32);
  assert_eq!(device.read(first + 1, 1).unwrap(), second as u32);
  assert_eq!(device.read(second + 1, 1).unwrap(), 0);
  device.write(first, 4, 0).unwrap();
  assert_eq!(device.read(first, 4).unwrap(), 0x01054809);
  assert_eq!(device.read(6, 2).unwrap(), 0x10);
  device.write(6, 2, u32::MAX).unwrap();
  assert_eq!(device.read(6, 2).unwrap(), 0x10);
  assert!(device.add_vendor_capability(&[9, 0, 5, 1]).is_err());
  assert!(device.add_vendor_capability(&[1, 0, 4, 1]).is_err());
  assert!(device.add_vendor_capability(&[9; 240]).is_err());
}
