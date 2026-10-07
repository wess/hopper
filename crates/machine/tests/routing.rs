#[allow(dead_code)]
mod support;

use machine::devices::virtio::{bus, input, pci};
use support::Ram;

fn device() -> pci::Device {
  pci::controller(input::create(input::Kind::Keyboard)).unwrap()
}

#[test]
fn configuration_does_not_alias_absent_functions_or_devices() {
  let mut devices = [None, Some(device())];
  let mut ram = Ram::default();
  assert_eq!(bus::read(&mut devices, 1 << 15, 2).unwrap(), 0x1af4);
  for offset in [0, (1 << 15) + 0x1000, 2 << 15, (31 << 15) + 0x7000] {
    for width in [1, 2, 4] {
      assert_eq!(
        bus::read(&mut devices, offset, width).unwrap(),
        u32::MAX >> ((4 - width) * 8)
      );
      bus::write(&mut devices, &mut ram, offset + 4, width, 6).unwrap();
    }
  }
  assert_eq!(bus::read(&mut devices, (1 << 15) + 4, 2).unwrap(), 0);
  for (offset, width) in [
    (1 << 20, 4),
    (u64::MAX, 4),
    (1, 2),
    (0xfff, 4),
    (0, 8),
    (0, 0),
  ] {
    assert!(bus::read(&mut devices, offset, width).is_err());
    assert!(bus::write(&mut devices, &mut ram, offset, width, 0).is_err());
  }
}

fn map(devices: &mut [Option<pci::Device>], ram: &mut Ram, slot: usize, address: u32) {
  let offset = (slot as u64) << 15;
  bus::write(devices, ram, offset + 0x10, 4, address).unwrap();
  bus::write(devices, ram, offset + 4, 2, 6).unwrap();
}

#[test]
fn memory_dispatch_tracks_bar_relocation_and_disabled_decoding() {
  let mut devices = [Some(device()), None, Some(device())];
  let mut ram = Ram::default();
  map(&mut devices, &mut ram, 2, 0x20000000);
  assert_eq!(bus::mapped(&devices, 0x20000000).unwrap(), Some(2));
  assert_eq!(
    bus::memory_write(&mut devices, &mut ram, 0x20000014, 1, 1).unwrap(),
    2
  );
  assert_eq!(
    bus::memory_read(&mut devices, 0x20000014, 1).unwrap(),
    (2, 1)
  );
  assert!(bus::memory_read(&mut devices, 0x20004000, 1).is_err());
  map(&mut devices, &mut ram, 2, 0x20008000);
  assert_eq!(bus::mapped(&devices, 0x20000000).unwrap(), None);
  assert_eq!(
    bus::memory_read(&mut devices, 0x20008014, 1).unwrap(),
    (2, 1)
  );
  bus::write(&mut devices, &mut ram, (2 << 15) + 4, 2, 0).unwrap();
  assert_eq!(bus::mapped(&devices, 0x20008000).unwrap(), None);
  assert!(bus::memory_write(&mut devices, &mut ram, 0x20008014, 1, 0).is_err());
}

#[test]
fn overlapping_devices_fail_before_register_mutation() {
  let mut devices = [Some(device()), Some(device())];
  let mut ram = Ram::default();
  map(&mut devices, &mut ram, 0, 0x20000000);
  map(&mut devices, &mut ram, 1, 0x20000000);
  assert!(bus::mapped(&devices, 0x20000014).is_err());
  assert!(bus::memory_write(&mut devices, &mut ram, 0x20000014, 1, 1).is_err());
  assert!(bus::memory_read(&mut devices, 0x20000014, 1).is_err());
  for device in devices.iter_mut().flatten() {
    assert_eq!(pci::read(device, 20, 1).unwrap(), 0);
  }
}

#[test]
fn high_memory_bars_and_eight_byte_msix_access_keep_full_values() {
  let mut devices = [Some(device())];
  let mut ram = Ram::default();
  map(&mut devices, &mut ram, 0, 0x20000000);
  bus::write(&mut devices, &mut ram, 0x14, 4, 1).unwrap();
  assert_eq!(bus::mapped(&devices, 0x20000000).unwrap(), None);
  assert_eq!(
    bus::memory_write(&mut devices, &mut ram, 0x1_20000014, 1, 1).unwrap(),
    0
  );
  assert_eq!(
    bus::memory_read(&mut devices, 0x1_20000014, 1).unwrap(),
    (0, 1)
  );
  bus::write(&mut devices, &mut ram, 0x18, 4, 0x20010000).unwrap();
  bus::memory_write(&mut devices, &mut ram, 0x20010000, 8, 0x12345678_30000040).unwrap();
  assert_eq!(
    bus::memory_read(&mut devices, 0x20010000, 8).unwrap(),
    (0, 0x12345678_30000040)
  );
  assert!(bus::memory_read(&mut devices, 0x20010001, 8).is_err());
  assert!(bus::memory_read(&mut devices, 0x20011000, 8).is_err());
}
