#[path = "support/disk.rs"]
mod disk;
#[path = "../examples/support/input.rs"]
mod keyboard;
#[path = "../examples/support/disk.rs"]
mod target;
use machine::devices::virtio::pci;

#[test]
fn disposable_target_refuses_existing_data_and_exposes_writable_capacity() {
  let image = disk::image(b"existing VM data");
  assert!(target::create(&image.0).is_err());
  assert_eq!(std::fs::read(&image.0).unwrap(), b"existing VM data");
  std::fs::remove_file(&image.0).unwrap();
  let mut device = target::create(&image.0).unwrap();
  assert_eq!(std::fs::metadata(&image.0).unwrap().len(), 64 * 1024 * 1024);
  assert_eq!(pci::features(&device) & pci::READONLY, 0);
  assert_eq!(pci::read(&mut device, pci::SPECIFIC, 4).unwrap(), 131072);
}

#[test]
fn diagnostic_digits_press_and_release_standard_keyboard_codes() {
  let events = keyboard::text(b"0123456789").unwrap();
  for (index, code) in [11, 2, 3, 4, 5, 6, 7, 8, 9, 10].into_iter().enumerate() {
    let key = &events[index * 4..index * 4 + 4];
    assert_eq!((key[0].kind, key[0].code, key[0].value), (1, code, 1));
    assert_eq!((key[2].kind, key[2].code, key[2].value), (1, code, 0));
    assert_eq!(key[1], machine::devices::virtio::input::SYN);
    assert_eq!(key[3], machine::devices::virtio::input::SYN);
  }
}

#[test]
fn diagnostic_command_switches_use_standard_keyboard_codes() {
  let events = keyboard::text(b"/-").unwrap();
  assert_eq!((events[0].code, events[4].code), (53, 12));
  assert_eq!((events[2].code, events[6].code), (53, 12));
  assert_eq!((events[2].value, events[6].value), (0, 0));
}
