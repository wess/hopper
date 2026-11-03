mod support;
use machine::devices::virtio::{
  console, gpu,
  input::{self, Event, Kind, SYN},
  pci::{self, snapshot},
};
use support::{descriptor, Ram, BASE};

#[path = "transport/records.rs"]
mod records;

fn device() -> pci::Device {
  pci::controller(input::create(Kind::Keyboard)).unwrap()
}

fn initialize(device: &mut pci::Device, ram: &mut Ram) {
  pci::config_write(device, ram, 4, 2, 6).unwrap();
  pci::config_write(device, ram, 0x10, 4, 0x20000004).unwrap();
  for (offset, width, value) in [(20, 1, 3), (8, 4, 1), (12, 4, 1), (20, 1, 11), (24, 2, 8)] {
    pci::write(device, ram, offset, width, value).unwrap();
  }
  for (offset, address) in [(32, BASE), (40, BASE + 0x100), (48, BASE + 0x200)] {
    pci::write(device, ram, offset, 4, address as u32).unwrap();
  }
  pci::write(device, ram, 28, 2, 1).unwrap();
  pci::write(device, ram, 20, 1, 15).unwrap();
  for index in 0..8u16 {
    descriptor(ram, index, BASE + 0x1000 + u64::from(index) * 8, 8, 2, 0);
    ram.0[0x104 + index as usize * 2..0x106 + index as usize * 2]
      .copy_from_slice(&index.to_le_bytes());
  }
}

#[test]
fn restored_transport_preserves_queue_progress_bar_probes_and_legacy_interrupts() {
  let mut original = device();
  let mut ram = Ram::default();
  initialize(&mut original, &mut ram);
  ram.0[0x102..0x104].copy_from_slice(&2u16.to_le_bytes());
  pci::send_input(
    &mut original,
    &mut ram,
    &[
      Event {
        kind: 1,
        code: 30,
        value: 1,
      },
      SYN,
    ],
  )
  .unwrap();
  assert_eq!(pci::completed(&original), 2);
  assert!(pci::interrupt(&original));
  pci::config_write(&mut original, &mut ram, 0x18, 4, u32::MAX).unwrap();
  let saved = reopen(snapshot::capture(&original).unwrap());
  let bytes = ram.0.clone();
  drop(original);
  for _ in 0..2 {
    let mut restored = device();
    let mut ram = Ram(bytes.clone());
    snapshot::restore_transport(&saved, &mut restored, &ram).unwrap();
    assert_eq!(pci::completed(&restored), 2);
    assert!(pci::interrupt(&restored));
    assert_eq!(restored.pci.memory(0x20000000), Some((0, 0)));
    assert_eq!(
      pci::config_read(&mut restored, 0x18, 4).unwrap(),
      0xfffff000
    );
    assert_eq!(pci::read(&mut restored, pci::ISR, 1).unwrap(), 1);
    assert!(!pci::interrupt(&restored));
    ram.0[0x102..0x104].copy_from_slice(&4u16.to_le_bytes());
    pci::send_input(&mut restored, &mut ram, &[SYN, SYN]).unwrap();
    assert_eq!(pci::completed(&restored), 4);
    assert_eq!(&ram.0[0x202..0x204], &4u16.to_le_bytes());
    assert_eq!(&ram.0[0x1000..0x1008], &[1, 0, 30, 0, 1, 0, 0, 0]);
    assert_eq!(&ram.0[0x1010..0x1020], &[0; 16]);
  }
}

#[test]
fn rejection_precedes_destination_mutation() {
  let mut original = device();
  let mut ram = Ram::default();
  initialize(&mut original, &mut ram);
  pci::send_input(&mut original, &mut ram, &[SYN, SYN]).unwrap();
  let saved = reopen(snapshot::capture(&original).unwrap());
  let mut other = pci::graphics(gpu::create(640, 480).unwrap()).unwrap();
  assert!(snapshot::restore_transport(&saved, &mut other, &ram).is_err());
  assert_eq!(pci::read(&mut other, 20, 1).unwrap(), 0);
  assert_eq!(pci::completed(&other), 0);
  let mut destination = device();
  ram.0[0x202] = 99;
  assert!(snapshot::restore_transport(&saved, &mut destination, &ram).is_err());
  assert_eq!(pci::read(&mut destination, 20, 1).unwrap(), 0);
  assert!(!destination.pci.bus_master());
  assert_eq!(pci::completed(&destination), 0);
}

#[test]
fn masked_msi_delivers_once_after_restore_and_unmask() {
  let mut original = device();
  let mut ram = Ram::default();
  initialize(&mut original, &mut ram);
  let mut capability = pci::config_read(&mut original, 0x34, 1).unwrap() as usize;
  while pci::config_read(&mut original, capability, 1).unwrap() != 0x11 {
    capability = pci::config_read(&mut original, capability + 1, 1).unwrap() as usize;
    assert_ne!(capability, 0);
  }
  pci::config_write(&mut original, &mut ram, capability + 2, 2, 0x8000).unwrap();
  pci::write(&mut original, &mut ram, 26, 2, 0).unwrap();
  pci::memory_write(&mut original, &mut ram, 2, 0, 8, 0x30000040).unwrap();
  pci::memory_write(&mut original, &mut ram, 2, 8, 4, 64).unwrap();
  ram.0[0x102..0x104].copy_from_slice(&2u16.to_le_bytes());
  pci::send_input(&mut original, &mut ram, &[SYN, SYN]).unwrap();
  assert!(pci::messages(&mut original).is_empty());
  assert_eq!(
    pci::memory_read(&mut original, 2, pci::msix::PENDING, 8).unwrap(),
    1
  );
  let saved = reopen(snapshot::capture(&original).unwrap());
  drop(original);
  let mut restored = device();
  snapshot::restore_transport(&saved, &mut restored, &ram).unwrap();
  assert!(pci::messages(&mut restored).is_empty());
  pci::memory_write(&mut restored, &mut ram, 2, 12, 4, 0).unwrap();
  assert_eq!(
    pci::messages(&mut restored),
    vec![pci::msix::Message {
      address: 0x30000040,
      data: 64
    }]
  );
  assert!(pci::messages(&mut restored).is_empty());
}

#[test]
fn backend_restore_preserves_pending_events_and_releases_only_delivered_keys() {
  let mut original = device();
  let mut ram = Ram::default();
  initialize(&mut original, &mut ram);
  let key = |code| Event {
    kind: 1,
    code,
    value: 1,
  };
  ram.0[0x102..0x104].copy_from_slice(&2u16.to_le_bytes());
  pci::send_input(&mut original, &mut ram, &[key(30), SYN]).unwrap();
  pci::send_input(&mut original, &mut ram, &[key(42), SYN]).unwrap();
  pci::write(&mut original, &mut ram, pci::SPECIFIC, 2, 1).unwrap();
  let saved = reopen(snapshot::capture(&original).unwrap());
  let bytes = ram.0.clone();
  pci::write(&mut original, &mut ram, 20, 1, 0).unwrap();
  drop(original);
  for deliver_pending in [true, false] {
    let mut restored = device();
    let mut ram = Ram(bytes.clone());
    snapshot::restore(&saved, &mut restored, &ram).unwrap();
    assert_eq!(pci::read(&mut restored, pci::SPECIFIC + 2, 1).unwrap(), 15);
    if deliver_pending {
      ram.0[0x102..0x104].copy_from_slice(&4u16.to_le_bytes());
      pci::write(&mut restored, &mut ram, pci::NOTIFY, 2, 0).unwrap();
      assert_eq!(&ram.0[0x1010..0x1018], &[1, 0, 42, 0, 1, 0, 0, 0]);
    }
    pci::release_input(&mut restored, &mut ram).unwrap();
    let (available, offset) = if deliver_pending {
      (7u16, 0x1020)
    } else {
      (4, 0x1010)
    };
    ram.0[0x102..0x104].copy_from_slice(&available.to_le_bytes());
    pci::write(&mut restored, &mut ram, pci::NOTIFY, 2, 0).unwrap();
    assert_eq!(&ram.0[offset..offset + 8], &[1, 0, 30, 0, 0, 0, 0, 0]);
    if deliver_pending {
      assert_eq!(&ram.0[offset + 8..offset + 16], &[1, 0, 42, 0, 0, 0, 0, 0]);
    } else {
      assert_eq!(&ram.0[offset + 8..offset + 16], &[0; 8]);
    }
    assert_eq!(pci::completed(&restored), u64::from(available));
  }
  let mut tablet = pci::controller(input::create(Kind::Tablet)).unwrap();
  assert!(snapshot::restore(&saved, &mut tablet, &Ram(bytes)).is_err());
  assert_eq!(pci::read(&mut tablet, 20, 1).unwrap(), 0);
  assert!(!tablet.pci.bus_master());
}

#[test]
fn foreign_disk_recovery_leaves_destination_untouched() {
  let disk = |id| {
    let file = tempfile::tempfile().unwrap();
    file.set_len(512).unwrap();
    pci::create(machine::devices::virtio::block::attach(file, false, [id; 20]).unwrap()).unwrap()
  };
  let original = disk(b'd');
  let saved = reopen(snapshot::capture(&original).unwrap());
  let mut destination = disk(b'e');
  assert!(snapshot::restore(&saved, &mut destination, &Ram::default()).is_err());
  assert_eq!(pci::completed(&destination), 0);
  assert_eq!(pci::read(&mut destination, 20, 1).unwrap(), 0);
}

fn reopen(saved: snapshot::State) -> snapshot::State {
  let bytes = snapshot::wire::encode(&saved).unwrap();
  if let Some(backend) = snapshot::backendwire::encode(&saved).unwrap() {
    drop(saved);
    snapshot::records::decode(
      &bytes,
      snapshot::records::Contents::Bytes(&backend),
      &device(),
      || Ok(()),
    )
    .unwrap()
  } else {
    snapshot::wire::reopen(&bytes, saved).unwrap()
  }
}

#[test]
fn backend_records_reject_foreign_input_kind_and_console_name() {
  let keyboard = snapshot::capture(&device()).unwrap();
  let tablet = input::wire::encode(&input::create(Kind::Tablet)).unwrap();
  assert!(snapshot::backendwire::reopen(keyboard, &tablet).is_err());
  let serial = pci::serial(console::create("org.hopper.setup").unwrap()).unwrap();
  let saved = snapshot::capture(&serial).unwrap();
  let foreign = console::wire::encode(&console::create("org.hopper.other").unwrap()).unwrap();
  assert!(snapshot::backendwire::reopen(saved, &foreign).is_err());
}
