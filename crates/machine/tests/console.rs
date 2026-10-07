mod support;

use machine::{
  devices::virtio::{
    console, pci,
    queue::{Buffer, Chain},
  },
  dma::Span,
};
use support::{Ram, BASE};

fn chain(offset: u64, length: u32, writable: bool) -> Chain {
  Chain {
    head: 0,
    buffers: vec![Buffer {
      span: Span {
        address: BASE + offset,
        length,
      },
      writable,
    }],
  }
}

fn control(port: &mut console::Port, ram: &mut Ram, id: u32, event: u16, value: u16) {
  ram.0[..4].copy_from_slice(&id.to_le_bytes());
  ram.0[4..6].copy_from_slice(&event.to_le_bytes());
  ram.0[6..8].copy_from_slice(&value.to_le_bytes());
  assert_eq!(
    console::execute(port, ram, &chain(0, 8, false), 3).unwrap(),
    0
  );
}

fn opened(ram: &mut Ram) -> console::Port {
  let mut port = console::create("org.hopper.setup").unwrap();
  control(&mut port, ram, u32::MAX, 0, 1);
  control(&mut port, ram, 0, 3, 1);
  control(&mut port, ram, 0, 6, 1);
  port
}

#[test]
fn handshake_names_one_port_and_fragments_host_input_without_losing_bytes() {
  let mut ram = Ram::default();
  let mut port = console::create("org.hopper.setup").unwrap();
  assert!(console::send(&mut port, b"data").is_err());
  control(&mut port, &mut ram, u32::MAX, 0, 1);
  assert_eq!(
    console::execute(&mut port, &mut ram, &chain(64, 8, true), 2).unwrap(),
    8
  );
  assert_eq!(&ram.0[64..72], &[0, 0, 0, 0, 1, 0, 1, 0]);
  control(&mut port, &mut ram, 0, 3, 1);
  assert!(console::execute(&mut port, &mut ram, &chain(64, 8, true), 2).is_err());
  let count = console::execute(&mut port, &mut ram, &chain(64, 128, true), 2).unwrap();
  assert_eq!(&ram.0[64..72], &[0, 0, 0, 0, 7, 0, 1, 0]);
  assert_eq!(&ram.0[72..64 + count as usize], b"org.hopper.setup\0");
  console::execute(&mut port, &mut ram, &chain(64, 8, true), 2).unwrap();
  assert_eq!(&ram.0[64..72], &[0, 0, 0, 0, 6, 0, 1, 0]);
  assert!(!console::ready(&port, 2));
  control(&mut port, &mut ram, 0, 6, 1);
  console::send(&mut port, b"abcdef").unwrap();
  assert!(console::execute(&mut port, &mut ram, &chain(0x10000, 3, true), 0).is_err());
  let fragmented = Chain {
    head: 0,
    buffers: vec![
      Buffer {
        span: Span {
          address: BASE + 128,
          length: 2,
        },
        writable: true,
      },
      Buffer {
        span: Span {
          address: BASE + 256,
          length: 2,
        },
        writable: true,
      },
    ],
  };
  assert_eq!(
    console::execute(&mut port, &mut ram, &fragmented, 0).unwrap(),
    4
  );
  assert_eq!(&ram.0[128..130], b"ab");
  assert_eq!(&ram.0[256..258], b"cd");
  assert_eq!(
    console::execute(&mut port, &mut ram, &chain(128, 8, true), 0).unwrap(),
    2
  );
  assert_eq!(&ram.0[128..130], b"ef");
  assert!(!console::ready(&port, 0));
}

#[test]
fn transport_bounds_output_and_discards_queued_host_input_on_disconnect_or_reset() {
  let mut ram = Ram(vec![b'a'; console::LIMIT + 1024]);
  let mut port = opened(&mut ram);
  ram.0[1024..].fill(b'a');
  assert_eq!(
    console::execute(
      &mut port,
      &mut ram,
      &chain(1024, console::LIMIT as u32, false),
      1
    )
    .unwrap(),
    0
  );
  assert!(!console::ready(&port, 1));
  assert!(console::execute(&mut port, &mut ram, &chain(1024, 1, false), 1).is_err());
  assert_eq!(console::receive(&mut port), vec![b'a'; console::LIMIT]);
  assert!(console::ready(&port, 1));
  console::send(&mut port, &vec![b'b'; console::LIMIT]).unwrap();
  assert!(console::send(&mut port, b"extra").is_err());
  control(&mut port, &mut ram, 0, 6, 0);
  assert!(!console::opened(&port));
  control(&mut port, &mut ram, 0, 6, 1);
  assert!(!console::ready(&port, 0));
  console::send(&mut port, b"pending").unwrap();
  console::execute(&mut port, &mut ram, &chain(1024, 3, false), 1).unwrap();
  console::reset(&mut port);
  assert!(console::receive(&mut port).is_empty());
  assert!(!console::opened(&port));
  assert!(!console::ready(&port, 0));
  assert!(!console::ready(&port, 2));
}

#[test]
fn malformed_controls_and_buffer_directions_do_not_open_the_port() {
  for name in ["", "bad/name", "bad\\name", "bad\0name", &"a".repeat(65)] {
    assert!(console::create(name).is_err());
  }
  let mut ram = Ram::default();
  let mut port = console::create("org.hopper.setup").unwrap();
  assert!(console::execute(&mut port, &mut ram, &chain(0, 8, true), 3).is_err());
  assert!(console::execute(&mut port, &mut ram, &chain(0, 7, false), 3).is_err());
  assert!(console::execute(&mut port, &mut ram, &chain(0, 8, false), 9).is_err());
  ram.0[4..6].copy_from_slice(&6u16.to_le_bytes());
  ram.0[6..8].copy_from_slice(&1u16.to_le_bytes());
  assert!(console::execute(&mut port, &mut ram, &chain(0, 8, false), 3).is_err());
  assert!(!console::opened(&port));
  control(&mut port, &mut ram, 0, 0, 1);
  ram.0[..4].copy_from_slice(&1u32.to_le_bytes());
  ram.0[4..6].copy_from_slice(&3u16.to_le_bytes());
  assert!(console::execute(&mut port, &mut ram, &chain(0, 8, false), 3).is_err());
  assert!(!console::opened(&port));
}

fn queue(device: &mut pci::Device, ram: &mut Ram, index: u32) {
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

fn descriptor(ram: &mut Ram, queue: usize, head: usize, address: u64, length: u32, writable: bool) {
  let start = queue * 0x400 + head * 16;
  ram.0[start..start + 8].copy_from_slice(&address.to_le_bytes());
  ram.0[start + 8..start + 12].copy_from_slice(&length.to_le_bytes());
  ram.0[start + 12..start + 14].copy_from_slice(&(if writable { 2u16 } else { 0 }).to_le_bytes());
}

#[test]
fn pci_control_replies_complete_posted_receive_buffers_and_require_multiport() {
  let mut ram = Ram::default();
  let mut device = pci::serial(console::create("org.hopper.setup").unwrap()).unwrap();
  assert_eq!(pci::config_read(&mut device, 0, 4).unwrap(), 0x10431af4);
  assert_eq!(pci::read(&mut device, pci::SPECIFIC + 4, 4).unwrap(), 1);
  pci::config_write(&mut device, &mut ram, 4, 2, 6).unwrap();
  pci::write(&mut device, &mut ram, 20, 1, 3).unwrap();
  pci::write(&mut device, &mut ram, 8, 4, 1).unwrap();
  pci::write(&mut device, &mut ram, 12, 4, 1).unwrap();
  pci::write(&mut device, &mut ram, 20, 1, 11).unwrap();
  assert_eq!(pci::read(&mut device, 20, 1).unwrap(), 3);
  pci::write(&mut device, &mut ram, 8, 4, 0).unwrap();
  pci::write(&mut device, &mut ram, 12, 4, console::MULTIPORT as u32).unwrap();
  pci::write(&mut device, &mut ram, 20, 1, 11).unwrap();
  for index in 0..4 {
    queue(&mut device, &mut ram, index);
  }
  pci::write(&mut device, &mut ram, 20, 1, 15).unwrap();
  for head in 0..3 {
    descriptor(
      &mut ram,
      2,
      head,
      BASE + 0x4000 + head as u64 * 128,
      128,
      true,
    );
    ram.0[0x904 + head * 2..0x906 + head * 2].copy_from_slice(&(head as u16).to_le_bytes());
    descriptor(&mut ram, 3, head, BASE + 0x5000 + head as u64 * 8, 8, false);
    ram.0[0xd04 + head * 2..0xd06 + head * 2].copy_from_slice(&(head as u16).to_le_bytes());
  }
  ram.0[0x902..0x904].copy_from_slice(&3u16.to_le_bytes());
  for (head, event) in [0u16, 3, 6].into_iter().enumerate() {
    let start = 0x5000 + head * 8;
    ram.0[start + 4..start + 6].copy_from_slice(&event.to_le_bytes());
    ram.0[start + 6..start + 8].copy_from_slice(&1u16.to_le_bytes());
    ram.0[0xd02..0xd04].copy_from_slice(&(head as u16 + 1).to_le_bytes());
    pci::write(&mut device, &mut ram, pci::NOTIFY + 12, 2, 3).unwrap();
  }
  assert_eq!(&ram.0[0xa02..0xa04], &3u16.to_le_bytes());
  assert_eq!(&ram.0[0x4088..0x4099], b"org.hopper.setup\0");
  support::descriptor(&mut ram, 0, BASE + 0x6000, 8, 2, 0);
  ram.0[0x102..0x104].copy_from_slice(&1u16.to_le_bytes());
  pci::send_serial(&mut device, &mut ram, b"host").unwrap();
  assert_eq!(&ram.0[0x6000..0x6004], b"host");
  descriptor(&mut ram, 1, 0, BASE + 0x7000, 5, false);
  ram.0[0x7000..0x7005].copy_from_slice(b"guest");
  ram.0[0x502..0x504].copy_from_slice(&1u16.to_le_bytes());
  pci::write(&mut device, &mut ram, pci::NOTIFY + 4, 2, 1).unwrap();
  assert_eq!(
    pci::receive_serial(&mut device, &mut ram).unwrap(),
    b"guest"
  );
  assert!(pci::interrupt(&device));
  assert!(pci::fault(&device).is_none());
  pci::write(&mut device, &mut ram, 20, 1, 0).unwrap();
  assert!(pci::send_serial(&mut device, &mut ram, b"old").is_err());
}
