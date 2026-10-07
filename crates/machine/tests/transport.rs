#[path = "support/disk.rs"]
mod disk;
mod support;

use machine::devices::virtio::{block, pci};
use support::{descriptor, Ram, BASE};

fn device() -> (disk::Image, pci::Device, Ram) {
  let image = disk::image(&[b'a'; 1024]);
  let device = pci::create(block::open(&image.0, false, [b'd'; 20]).unwrap()).unwrap();
  (image, device, Ram::default())
}

fn initialize(device: &mut pci::Device, ram: &mut Ram) {
  pci::config_write(device, ram, 4, 2, 6).unwrap();
  pci::config_write(device, ram, 0x10, 4, 0x20000004).unwrap();
  pci::write(device, ram, 20, 1, 1).unwrap();
  pci::write(device, ram, 20, 1, 3).unwrap();
  pci::write(device, ram, 8, 4, 1).unwrap();
  pci::write(device, ram, 12, 4, 1).unwrap();
  pci::write(device, ram, 8, 4, 0).unwrap();
  pci::write(device, ram, 12, 4, (pci::FLUSH | pci::BLOCK_SIZE) as u32).unwrap();
  pci::write(device, ram, 20, 1, 11).unwrap();
  assert_eq!(pci::read(device, 20, 1).unwrap(), 11);
  pci::write(device, ram, 24, 2, 8).unwrap();
  for (offset, address) in [(32, BASE), (40, BASE + 0x100), (48, BASE + 0x200)] {
    pci::write(device, ram, offset, 4, address as u32).unwrap();
    pci::write(device, ram, offset + 4, 4, (address >> 32) as u32).unwrap();
  }
  pci::write(device, ram, 28, 2, 1).unwrap();
  pci::write(device, ram, 20, 1, 15).unwrap();
}

fn submit(ram: &mut Ram) {
  descriptor(ram, 0, BASE + 0x1000, 16, 1, 1);
  descriptor(ram, 1, BASE + 0x2000, 512, 3, 2);
  descriptor(ram, 2, BASE + 0x4000, 1, 2, 0);
  ram.0[0x4000] = 0xff;
  ram.0[0x102..0x104].copy_from_slice(&1u16.to_le_bytes());
}

#[test]
fn modern_pci_transport_discovers_and_reads_a_disk() {
  let (_image, mut device, mut ram) = device();
  assert_eq!(pci::config_read(&mut device, 0, 4).unwrap(), 0x10421af4);
  assert_eq!(pci::read(&mut device, pci::SPECIFIC, 4).unwrap(), 2);
  assert_eq!(pci::read(&mut device, pci::SPECIFIC + 20, 4).unwrap(), 512);
  initialize(&mut device, &mut ram);
  assert_eq!(device.pci.memory(0x20000000), Some((0, 0)));
  assert_eq!(pci::read(&mut device, 28, 2).unwrap(), 1);
  submit(&mut ram);
  pci::write(&mut device, &mut ram, pci::NOTIFY, 2, 0).unwrap();
  assert_eq!(pci::completed(&device), 1);
  assert_eq!(ram.0[0x4000], 0);
  assert_eq!(&ram.0[0x2000..0x2200], &[b'a'; 512]);
  assert_eq!(&ram.0[0x202..0x204], &1u16.to_le_bytes());
  assert!(pci::interrupt(&device));
  assert_eq!(pci::config_read(&mut device, 6, 2).unwrap() & 8, 8);
  pci::config_write(&mut device, &mut ram, 4, 2, 0x406).unwrap();
  assert!(!pci::interrupt(&device));
  pci::config_write(&mut device, &mut ram, 4, 2, 6).unwrap();
  assert!(pci::interrupt(&device));
  assert_eq!(pci::read(&mut device, pci::ISR, 1).unwrap(), 1);
  assert!(!pci::interrupt(&device));
  assert_eq!(pci::config_read(&mut device, 6, 2).unwrap() & 8, 0);
  pci::write(&mut device, &mut ram, pci::NOTIFY, 2, 0).unwrap();
  assert_eq!(pci::completed(&device), 1);
}

#[test]
fn bus_master_and_reset_gate_guest_dma() {
  let (_image, mut device, mut ram) = device();
  ram.0[0x200..0x204].fill(0xcc);
  initialize(&mut device, &mut ram);
  assert_eq!(&ram.0[0x200..0x204], &[0xcc; 4]);
  submit(&mut ram);
  pci::config_write(&mut device, &mut ram, 4, 2, 2).unwrap();
  pci::write(&mut device, &mut ram, pci::NOTIFY, 2, 0).unwrap();
  assert_eq!(pci::completed(&device), 0);
  assert_eq!(ram.0[0x4000], 0xff);
  assert_eq!(&ram.0[0x200..0x204], &[0xcc; 4]);
  pci::config_write(&mut device, &mut ram, 4, 2, 6).unwrap();
  pci::write(&mut device, &mut ram, pci::NOTIFY, 2, 0).unwrap();
  assert_eq!(pci::completed(&device), 1);
  pci::write(&mut device, &mut ram, 20, 1, 0).unwrap();
  assert_eq!(pci::read(&mut device, 20, 1).unwrap(), 0);
  assert_eq!(pci::read(&mut device, 28, 2).unwrap(), 0);
  assert_eq!(pci::read(&mut device, 24, 2).unwrap(), 256);
  assert!(!pci::interrupt(&device));
  pci::write(&mut device, &mut ram, pci::NOTIFY, 2, 0).unwrap();
  assert_eq!(pci::completed(&device), 1);
}

#[test]
fn unsupported_features_and_invalid_guest_queues_are_recoverable() {
  let (_image, mut device, mut ram) = device();
  pci::write(&mut device, &mut ram, 0, 4, u32::MAX).unwrap();
  assert_eq!(pci::read(&mut device, 4, 4).unwrap(), 0);
  pci::write(&mut device, &mut ram, 20, 1, 3).unwrap();
  pci::write(&mut device, &mut ram, 8, 4, 0).unwrap();
  pci::write(&mut device, &mut ram, 12, 4, 1 << 31).unwrap();
  pci::write(&mut device, &mut ram, 20, 1, 15).unwrap();
  assert_eq!(pci::read(&mut device, 20, 1).unwrap(), 3);
  pci::write(&mut device, &mut ram, 28, 2, 1).unwrap();
  assert_eq!(pci::read(&mut device, 20, 1).unwrap() & 64, 64);
  assert!(pci::fault(&device).is_some());
  assert_eq!(pci::read(&mut device, pci::ISR, 1).unwrap(), 2);
  pci::write(&mut device, &mut ram, 20, 1, 0).unwrap();
  assert!(pci::fault(&device).is_none());
  pci::write(&mut device, &mut ram, 22, 2, 1).unwrap();
  assert_eq!(pci::read(&mut device, 24, 2).unwrap(), 0);
  pci::write(&mut device, &mut ram, 22, 2, 0).unwrap();
  initialize(&mut device, &mut ram);
  pci::write(&mut device, &mut ram, 32, 4, u32::MAX).unwrap();
  assert_eq!(pci::read(&mut device, 32, 4).unwrap(), BASE as u32);
}

#[test]
fn configuration_window_executes_real_register_accesses() {
  let (_image, mut device, mut ram) = device();
  let mut offset = pci::config_read(&mut device, 0x34, 1).unwrap() as usize;
  let mut kinds = Vec::new();
  loop {
    let kind = pci::config_read(&mut device, offset + 3, 1).unwrap();
    kinds.push(kind);
    if kind == 5 {
      break;
    }
    offset = pci::config_read(&mut device, offset + 1, 1).unwrap() as usize;
    assert_ne!(offset, 0);
  }
  assert_eq!(kinds, [1, 2, 3, 4, 5]);
  pci::config_write(&mut device, &mut ram, offset + 8, 4, pci::SPECIFIC as u32).unwrap();
  pci::config_write(&mut device, &mut ram, offset + 12, 4, 4).unwrap();
  assert_eq!(pci::config_read(&mut device, offset + 16, 4).unwrap(), 2);
  pci::config_write(&mut device, &mut ram, offset + 8, 4, 20).unwrap();
  pci::config_write(&mut device, &mut ram, offset + 12, 4, 1).unwrap();
  pci::config_write(&mut device, &mut ram, offset + 16, 4, 0xffff0003).unwrap();
  assert_eq!(pci::read(&mut device, 20, 1).unwrap(), 3);
  pci::config_write(&mut device, &mut ram, offset + 4, 1, 1).unwrap();
  assert_eq!(pci::config_read(&mut device, offset + 16, 1).unwrap(), 0);
}
