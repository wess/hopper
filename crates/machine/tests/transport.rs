#[path = "support/disk.rs"]
mod disk;
mod support;

use machine::devices::virtio::{block, gpu, pci};
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
fn disk_completion_uses_msix_masks_bus_mastering_and_reset() {
  let (_image, mut device, mut ram) = device();
  initialize(&mut device, &mut ram);
  let mut cap = pci::config_read(&mut device, 0x34, 1).unwrap() as usize;
  while pci::config_read(&mut device, cap, 1).unwrap() != 0x11 {
    cap = pci::config_read(&mut device, cap + 1, 1).unwrap() as usize;
    assert_ne!(cap, 0);
  }
  pci::write(&mut device, &mut ram, 16, 2, 0).unwrap();
  assert_eq!(pci::read(&mut device, 16, 2).unwrap(), 0);
  pci::write(&mut device, &mut ram, 26, 2, 2).unwrap();
  assert_eq!(pci::read(&mut device, 26, 2).unwrap(), 0xffff);
  pci::write(&mut device, &mut ram, 26, 2, 1).unwrap();
  pci::config_write(&mut device, &mut ram, cap + 2, 2, 0x8000).unwrap();
  pci::memory_write(&mut device, &mut ram, 2, 16, 8, 0x30000040).unwrap();
  pci::memory_write(&mut device, &mut ram, 2, 24, 4, 64).unwrap();
  submit(&mut ram);
  pci::write(&mut device, &mut ram, pci::NOTIFY, 2, 0).unwrap();
  assert_eq!(pci::completed(&device), 1);
  assert!(!pci::interrupt(&device));
  assert_eq!(pci::config_read(&mut device, 6, 2).unwrap() & 8, 0);
  assert!(pci::messages(&mut device).is_empty());
  assert_eq!(
    pci::memory_read(&mut device, 2, pci::msix::PENDING, 8).unwrap(),
    2
  );
  pci::memory_write(&mut device, &mut ram, 2, 28, 4, 0).unwrap();
  pci::config_write(&mut device, &mut ram, 4, 2, 2).unwrap();
  assert!(pci::messages(&mut device).is_empty());
  pci::config_write(&mut device, &mut ram, 4, 2, 6).unwrap();
  assert_eq!(
    pci::messages(&mut device),
    vec![pci::msix::Message {
      address: 0x30000040,
      data: 64,
    }]
  );
  assert!(pci::messages(&mut device).is_empty());
  assert_eq!(
    pci::memory_read(&mut device, 2, pci::msix::PENDING, 8).unwrap(),
    0
  );
  pci::write(&mut device, &mut ram, 20, 1, 0).unwrap();
  assert_eq!(pci::read(&mut device, 16, 2).unwrap(), 0xffff);
  assert_eq!(pci::read(&mut device, 26, 2).unwrap(), 0xffff);
  assert!(device.pci.msix_enabled());
  assert_eq!(pci::memory_read(&mut device, 2, 16, 8).unwrap(), 0x30000040);
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

#[test]
fn gpu_control_and_cursor_queues_keep_independent_configuration_and_notifications() {
  let mut device = pci::graphics(gpu::create(1024, 768).unwrap()).unwrap();
  let mut ram = Ram::default();
  assert_eq!(pci::config_read(&mut device, 0, 4).unwrap(), 0x10501af4);
  assert_eq!(pci::features(&device), pci::VERSION);
  assert_eq!(pci::read(&mut device, 18, 2).unwrap(), 2);
  assert_eq!(pci::read(&mut device, pci::SPECIFIC + 8, 4).unwrap(), 1);
  assert_eq!(pci::read(&mut device, pci::SPECIFIC + 12, 4).unwrap(), 0);
  pci::config_write(&mut device, &mut ram, 4, 2, 6).unwrap();
  pci::write(&mut device, &mut ram, 20, 1, 3).unwrap();
  pci::write(&mut device, &mut ram, 8, 4, 1).unwrap();
  pci::write(&mut device, &mut ram, 12, 4, 1).unwrap();
  pci::write(&mut device, &mut ram, 20, 1, 11).unwrap();
  for index in 0..2 {
    pci::write(&mut device, &mut ram, 22, 2, index).unwrap();
    assert_eq!(pci::read(&mut device, 30, 2).unwrap(), index);
    pci::write(&mut device, &mut ram, 24, 2, 8).unwrap();
    for (offset, address) in [(32, BASE), (40, BASE + 0x100), (48, BASE + 0x200)] {
      pci::write(
        &mut device,
        &mut ram,
        offset,
        4,
        address as u32 + index * 0x400,
      )
      .unwrap();
    }
    pci::write(&mut device, &mut ram, 28, 2, 1).unwrap();
  }
  pci::write(&mut device, &mut ram, 20, 1, 15).unwrap();
  ram.0[0x1000..0x1004].copy_from_slice(&0x100u32.to_le_bytes());
  descriptor(&mut ram, 0, BASE + 0x1000, 24, 1, 1);
  descriptor(&mut ram, 1, BASE + 0x2000, 408, 2, 0);
  ram.0[0x102..0x104].copy_from_slice(&1u16.to_le_bytes());
  ram.0[0x3000..0x3004].copy_from_slice(&0x300u32.to_le_bytes());
  ram.0[0x301c..0x3020].copy_from_slice(&150u32.to_le_bytes());
  ram.0[0x3020..0x3024].copy_from_slice(&250u32.to_le_bytes());
  descriptor(&mut ram, 2, BASE + 0x3000, 56, 0, 0);
  let cursor_descriptor = ram.0[32..48].to_vec();
  ram.0[0x400..0x410].copy_from_slice(&cursor_descriptor);
  ram.0[0x502..0x504].copy_from_slice(&1u16.to_le_bytes());
  pci::write(&mut device, &mut ram, pci::NOTIFY + 4, 2, 0).unwrap();
  assert_eq!(pci::completed(&device), 0);
  pci::write(&mut device, &mut ram, pci::NOTIFY + 4, 2, 1).unwrap();
  assert_eq!(pci::completed(&device), 1);
  let cursor = gpu::cursor(pci::display(&device).unwrap());
  assert_eq!((cursor.x, cursor.y), (150, 250));
  assert_eq!(&ram.0[0x602..0x604], &1u16.to_le_bytes());
  assert_eq!(&ram.0[0x202..0x204], &[0, 0]);
  pci::write(&mut device, &mut ram, pci::NOTIFY, 2, 0).unwrap();
  assert_eq!(pci::completed(&device), 2);
  assert_eq!(&ram.0[0x2000..0x2004], &0x1101u32.to_le_bytes());
  assert_eq!(&ram.0[0x2020..0x2028], &[0, 4, 0, 0, 0, 3, 0, 0]);
  assert!(pci::fault(&device).is_none());
  pci::write(&mut device, &mut ram, 22, 2, 0).unwrap();
  assert_eq!(pci::read(&mut device, 32, 4).unwrap(), BASE as u32);
  pci::write(&mut device, &mut ram, 20, 1, 0).unwrap();
  for index in 0..2 {
    pci::write(&mut device, &mut ram, 22, 2, index).unwrap();
    assert_eq!(pci::read(&mut device, 24, 2).unwrap(), 256);
    assert_eq!(pci::read(&mut device, 28, 2).unwrap(), 0);
    assert_eq!(pci::read(&mut device, 32, 4).unwrap(), 0);
  }
  assert_eq!(gpu::cursor(pci::display(&device).unwrap()).generation, 0);
}
