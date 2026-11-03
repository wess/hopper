mod support;
use machine::{
  devices::virtio::{
    console,
    pci::{self, snapshot},
    queue::{Buffer, Chain},
  },
  dma::Span,
};
use support::{Ram, BASE};

fn chain(offset: u64, length: u32) -> Chain {
  Chain {
    head: 0,
    buffers: vec![Buffer {
      span: Span {
        address: BASE + offset,
        length,
      },
      writable: false,
    }],
  }
}

fn port(ram: &mut Ram) -> console::Port {
  let mut port = console::create("org.hopper.setup").unwrap();
  for event in [0u16, 3, 6] {
    ram.0[..4].fill(0);
    ram.0[4..6].copy_from_slice(&event.to_le_bytes());
    ram.0[6..8].copy_from_slice(&1u16.to_le_bytes());
    console::execute(&mut port, ram, &chain(0, 8), 3).unwrap();
  }
  console::send(&mut port, b"abcdef").unwrap();
  ram.0[0x1000..0x1005].copy_from_slice(b"guest");
  console::execute(&mut port, ram, &chain(0x1000, 5), 1).unwrap();
  port
}

fn initialize(device: &mut pci::Device, ram: &mut Ram) {
  pci::config_write(device, ram, 4, 2, 6).unwrap();
  for (offset, width, value) in [
    (20, 1, 3),
    (8, 4, 1),
    (12, 4, 1),
    (8, 4, 0),
    (12, 4, console::MULTIPORT as u32),
    (20, 1, 11),
  ] {
    pci::write(device, ram, offset, width, value).unwrap();
  }
  for index in [0u32, 2] {
    pci::write(device, ram, 22, 2, index).unwrap();
    pci::write(device, ram, 24, 2, 8).unwrap();
    for (offset, address) in [(32, BASE), (40, BASE + 0x100), (48, BASE + 0x200)] {
      pci::write(
        device,
        ram,
        offset,
        4,
        (address + u64::from(index) * 0x400) as u32,
      )
      .unwrap();
    }
    pci::write(device, ram, 28, 2, 1).unwrap();
  }
  pci::write(device, ram, 20, 1, 15).unwrap();
}

fn receive_buffer(ram: &mut Ram, index: usize, head: u16, address: u64, length: u32) {
  let offset = index * 0x400 + head as usize * 16;
  if index == 0 {
    support::descriptor(ram, head, address, length, 2, 0);
  } else {
    ram.0[offset..offset + 8].copy_from_slice(&address.to_le_bytes());
    ram.0[offset + 8..offset + 12].copy_from_slice(&length.to_le_bytes());
    ram.0[offset + 12..offset + 14].copy_from_slice(&2u16.to_le_bytes());
  }
  let available = index * 0x400 + 0x104 + head as usize * 2;
  ram.0[available..available + 2].copy_from_slice(&head.to_le_bytes());
}

#[test]
fn restored_serial_port_retains_negotiation_controls_and_both_byte_directions() {
  let mut ram = Ram::default();
  let mut original = pci::serial(port(&mut ram)).unwrap();
  let saved = snapshot::capture(&original).unwrap();
  let transport = snapshot::wire::encode(&saved).unwrap();
  let bytes = snapshot::backendwire::encode(&saved).unwrap().unwrap();
  drop(saved);
  let target = pci::serial(console::create("org.hopper.setup").unwrap()).unwrap();
  let saved = snapshot::records::decode(
    &transport,
    snapshot::records::Contents::Bytes(&bytes),
    &target,
    || Ok(()),
  )
  .unwrap();
  pci::write(&mut original, &mut ram, 20, 1, 0).unwrap();
  drop(original);
  for _ in 0..2 {
    let mut restored = pci::serial(console::create("org.hopper.setup").unwrap()).unwrap();
    let mut ram = Ram::default();
    snapshot::restore(&saved, &mut restored, &ram).unwrap();
    assert_eq!(
      pci::receive_serial(&mut restored, &mut ram).unwrap(),
      b"guest"
    );
    assert!(pci::receive_serial(&mut restored, &mut ram)
      .unwrap()
      .is_empty());
    initialize(&mut restored, &mut ram);
    for head in 0..3u16 {
      receive_buffer(
        &mut ram,
        2,
        head,
        BASE + 0x7000 + u64::from(head) * 128,
        128,
      );
    }
    ram.0[0x902..0x904].copy_from_slice(&3u16.to_le_bytes());
    pci::write(&mut restored, &mut ram, pci::NOTIFY + 8, 2, 2).unwrap();
    assert_eq!(&ram.0[0x7000..0x7008], &[0, 0, 0, 0, 1, 0, 1, 0]);
    assert_eq!(&ram.0[0x7088..0x7099], b"org.hopper.setup\0");
    assert_eq!(&ram.0[0x7100..0x7108], &[0, 0, 0, 0, 6, 0, 1, 0]);
    receive_buffer(&mut ram, 0, 0, BASE + 0x6000, 3);
    receive_buffer(&mut ram, 0, 1, BASE + 0x6008, 8);
    ram.0[0x102..0x104].copy_from_slice(&2u16.to_le_bytes());
    pci::send_serial(&mut restored, &mut ram, b"tail").unwrap();
    assert_eq!(&ram.0[0x6000..0x6003], b"abc");
    assert_eq!(&ram.0[0x6008..0x600f], b"deftail");
    assert_eq!(pci::completed(&restored), 5);
  }
  let mut foreign = pci::serial(console::create("org.hopper.other").unwrap()).unwrap();
  assert!(snapshot::restore(&saved, &mut foreign, &Ram::default()).is_err());
  assert_eq!(pci::read(&mut foreign, 20, 1).unwrap(), 0);
  assert!(!foreign.pci.bus_master());
}

#[test]
fn console_records_reject_damage_truncation_and_rechecksummed_invalid_state() {
  use sha2::{Digest, Sha256};
  let mut ram = Ram::default();
  let bytes = console::wire::encode(&port(&mut ram)).unwrap();
  assert_eq!(
    console::wire::encode(&console::wire::decode(&bytes).unwrap()).unwrap(),
    bytes
  );
  for end in 0..bytes.len() {
    assert!(console::wire::decode(&bytes[..end]).is_err());
  }
  for offset in 0..bytes.len() {
    let mut changed = bytes.clone();
    changed[offset] ^= 1;
    assert!(console::wire::decode(&changed).is_err());
  }
  let flags = 5 + bytes[4] as usize;
  for (offset, value) in [
    (0, b'X'),
    (4, 65),
    (5, b'/'),
    (flags, 0),
    (flags + 1, 0),
    (flags + 2, 2),
    (flags + 3, 33),
    (flags + 4, 7),
    (flags + 5, 1),
  ] {
    let mut changed = bytes.clone();
    changed[offset] = value;
    let end = changed.len() - 32;
    let checksum = Sha256::digest(&changed[..end]);
    changed[end..].copy_from_slice(&checksum);
    assert!(
      console::wire::decode(&changed).is_err(),
      "Accepted invalid console field at {offset}"
    );
  }
  let mut extended = bytes.to_vec();
  extended.insert(extended.len() - 32, 0);
  let end = extended.len() - 32;
  let checksum = Sha256::digest(&extended[..end]);
  extended[end..].copy_from_slice(&checksum);
  assert!(console::wire::decode(&extended).is_err());
  assert!(console::wire::decode(&vec![0; console::wire::MAX + 1]).is_err());
}
#[test]
fn maximum_console_buffers_survive_reopening_and_drain_through_guest_queues() {
  let mut ram = Ram::default();
  let mut original = port(&mut ram);
  console::send(&mut original, &vec![b'a'; console::LIMIT - 6]).unwrap();
  ram.0[..console::LIMIT - 5].fill(b'g');
  console::execute(
    &mut original,
    &mut ram,
    &chain(0, (console::LIMIT - 5) as u32),
    1,
  )
  .unwrap();
  let bytes = console::wire::encode(&original).unwrap();
  drop(original);
  assert!(bytes.len() <= console::wire::MAX);
  let mut restored = console::wire::decode(&bytes).unwrap();
  let output = console::receive(&mut restored);
  assert_eq!(output.len(), console::LIMIT);
  assert_eq!(&output[..5], b"guest");
  let mut input = chain(0, console::LIMIT as u32);
  input.buffers[0].writable = true;
  assert_eq!(
    console::execute(&mut restored, &mut ram, &input, 0).unwrap(),
    console::LIMIT as u32
  );
  assert_eq!(&ram.0[..6], b"abcdef");
  assert!(ram.0[6..].iter().all(|byte| *byte == b'a'));
}
