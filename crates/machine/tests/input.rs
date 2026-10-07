mod support;

use machine::{
  devices::virtio::{
    input::{self, Event, Kind, SYN},
    pci,
    queue::{Buffer, Chain},
  },
  dma::Span,
};
use support::{descriptor, Ram, BASE};

fn chain(address: u64, length: u32, writable: bool) -> Chain {
  Chain {
    head: 0,
    buffers: vec![Buffer {
      span: Span { address, length },
      writable,
    }],
  }
}

#[test]
fn configuration_reports_only_supported_capabilities_and_ignores_guest_payload_writes() {
  let mut keyboard = input::create(Kind::Keyboard);
  input::configure(&mut keyboard, 0, 2, 1);
  let config = input::config(&keyboard);
  assert_eq!(config[2], 15);
  assert_eq!(&config[8..23], b"Hopper Keyboard");
  input::configure(&mut keyboard, 0, 2, 0x0111);
  let config = input::config(&keyboard);
  assert_eq!(config[2], 32);
  assert_eq!(config[8], 254);
  assert!(config[9..40].iter().all(|byte| *byte == 255));
  input::configure(&mut keyboard, 8, 4, 0);
  assert_eq!(input::config(&keyboard), config);
  input::configure(&mut keyboard, 1, 1, 3);
  assert_eq!(input::config(&keyboard)[2], 0);
  let mut pointer = input::create(Kind::Tablet);
  input::configure(&mut pointer, 1, 1, 1);
  input::configure(&mut pointer, 0, 1, 0x12);
  let config = input::config(&pointer);
  assert_eq!(config[2], 20);
  assert_eq!(&config[12..16], &65535u32.to_le_bytes());
  input::configure(&mut pointer, 0, 2, 0x0111);
  assert_eq!(input::config(&pointer)[8 + 34], 7);
}

#[test]
fn input_batches_are_bounded_and_release_discards_unsent_presses() {
  let mut keyboard = input::create(Kind::Keyboard);
  let mut ram = Ram::default();
  let press = Event {
    kind: 1,
    code: 42,
    value: 1,
  };
  assert!(input::enqueue(&mut keyboard, &[press]).is_err());
  assert!(input::enqueue(&mut keyboard, &[Event { code: 999, ..press }, SYN]).is_err());
  assert_eq!(input::pending(&keyboard), 0);
  input::enqueue(&mut keyboard, &[press, SYN]).unwrap();
  assert!(input::execute(&mut keyboard, &mut ram, &chain(BASE, 7, true), 0).is_err());
  assert_eq!(input::pending(&keyboard), 2);
  input::execute(&mut keyboard, &mut ram, &chain(BASE, 8, true), 0).unwrap();
  assert_eq!(&ram.0[..8], &[1, 0, 42, 0, 1, 0, 0, 0]);
  input::enqueue(&mut keyboard, &[Event { code: 30, ..press }, SYN]).unwrap();
  input::release(&mut keyboard);
  assert_eq!(input::pending(&keyboard), 2);
  input::execute(&mut keyboard, &mut ram, &chain(BASE, 8, true), 0).unwrap();
  assert_eq!(&ram.0[..8], &[1, 0, 42, 0, 0, 0, 0, 0]);
  input::execute(&mut keyboard, &mut ram, &chain(BASE, 8, true), 0).unwrap();
  assert_eq!(&ram.0[..8], &[0; 8]);
  input::release(&mut keyboard);
  assert_eq!(input::pending(&keyboard), 0);
  input::enqueue(&mut keyboard, &vec![SYN; 1024]).unwrap();
  assert!(input::enqueue(&mut keyboard, &[press, SYN]).is_err());
  input::reset(&mut keyboard);
  assert_eq!(input::pending(&keyboard), 0);
}

#[test]
fn pointer_coordinates_wheel_buttons_and_guest_led_feedback_follow_wire_layout() {
  let mut pointer = input::create(Kind::Tablet);
  let mut ram = Ram::default();
  let events = [
    Event {
      kind: 3,
      code: 0,
      value: 65535,
    },
    Event {
      kind: 3,
      code: 1,
      value: 32768,
    },
    Event {
      kind: 2,
      code: 8,
      value: -1,
    },
    Event {
      kind: 1,
      code: 272,
      value: 1,
    },
    SYN,
  ];
  input::enqueue(&mut pointer, &events).unwrap();
  for event in events {
    input::execute(&mut pointer, &mut ram, &chain(BASE, 8, true), 0).unwrap();
    assert_eq!(&ram.0[..2], &event.kind.to_le_bytes());
    assert_eq!(&ram.0[2..4], &event.code.to_le_bytes());
    assert_eq!(&ram.0[4..8], &event.value.to_le_bytes());
  }
  assert!(input::enqueue(
    &mut pointer,
    &[
      Event {
        kind: 3,
        code: 0,
        value: 65536
      },
      SYN
    ]
  )
  .is_err());
  input::reset(&mut pointer);
  input::enqueue(
    &mut pointer,
    &events[..2].iter().copied().chain([SYN]).collect::<Vec<_>>(),
  )
  .unwrap();
  input::execute(&mut pointer, &mut ram, &chain(BASE, 8, true), 0).unwrap();
  input::release(&mut pointer);
  assert_eq!(input::pending(&pointer), 1);
  input::execute(&mut pointer, &mut ram, &chain(BASE, 8, true), 0).unwrap();
  assert_eq!(&ram.0[..8], &[0; 8]);
  let mut keyboard = input::create(Kind::Keyboard);
  ram.0[..8].copy_from_slice(&[17, 0, 1, 0, 1, 0, 0, 0]);
  assert_eq!(
    input::execute(&mut keyboard, &mut ram, &chain(BASE, 8, false), 1).unwrap(),
    0
  );
  assert_eq!(input::leds(&keyboard), 2);
  ram.0[4] = 0;
  input::execute(&mut keyboard, &mut ram, &chain(BASE, 8, false), 1).unwrap();
  assert_eq!(input::leds(&keyboard), 0);
  assert!(input::execute(&mut keyboard, &mut ram, &chain(BASE, 9, false), 1).is_err());
}

#[test]
fn pci_input_waits_for_events_and_bus_master_before_consuming_receive_buffers() {
  let mut device = pci::controller(input::create(Kind::Keyboard)).unwrap();
  let mut ram = Ram::default();
  assert_eq!(pci::config_read(&mut device, 0, 4).unwrap(), 0x10521af4);
  pci::config_write(&mut device, &mut ram, 4, 2, 6).unwrap();
  pci::write(&mut device, &mut ram, 20, 1, 3).unwrap();
  pci::write(&mut device, &mut ram, 8, 4, 1).unwrap();
  pci::write(&mut device, &mut ram, 12, 4, 1).unwrap();
  pci::write(&mut device, &mut ram, 20, 1, 11).unwrap();
  pci::write(&mut device, &mut ram, 24, 2, 8).unwrap();
  for (offset, address) in [(32, BASE), (40, BASE + 0x100), (48, BASE + 0x200)] {
    pci::write(&mut device, &mut ram, offset, 4, address as u32).unwrap();
  }
  pci::write(&mut device, &mut ram, 28, 2, 1).unwrap();
  pci::write(&mut device, &mut ram, 20, 1, 15).unwrap();
  for index in 0..4 {
    descriptor(&mut ram, index, BASE + 0x1000 + index as u64 * 8, 8, 2, 0);
    ram.0[0x104 + index as usize * 2..0x106 + index as usize * 2]
      .copy_from_slice(&index.to_le_bytes());
  }
  ram.0[0x102..0x104].copy_from_slice(&4u16.to_le_bytes());
  pci::write(&mut device, &mut ram, pci::NOTIFY, 2, 0).unwrap();
  assert_eq!(pci::completed(&device), 0);
  pci::config_write(&mut device, &mut ram, 4, 2, 2).unwrap();
  pci::send_input(
    &mut device,
    &mut ram,
    &[
      Event {
        kind: 1,
        code: 42,
        value: 1,
      },
      SYN,
    ],
  )
  .unwrap();
  assert_eq!(pci::completed(&device), 0);
  pci::config_write(&mut device, &mut ram, 4, 2, 6).unwrap();
  pci::write(&mut device, &mut ram, pci::NOTIFY, 2, 0).unwrap();
  assert_eq!(pci::completed(&device), 2);
  assert_eq!(&ram.0[0x1000..0x1008], &[1, 0, 42, 0, 1, 0, 0, 0]);
  assert!(pci::interrupt(&device));
  pci::release_input(&mut device, &mut ram).unwrap();
  assert_eq!(pci::completed(&device), 4);
  assert_eq!(&ram.0[0x1010..0x1018], &[1, 0, 42, 0, 0, 0, 0, 0]);
  assert!(pci::fault(&device).is_none());
  pci::write(&mut device, &mut ram, pci::SPECIFIC, 2, 1).unwrap();
  assert_eq!(pci::read(&mut device, pci::SPECIFIC + 2, 1).unwrap(), 15);
}
