mod support;

use machine::{
  devices::virtio::queue,
  dma::{self, Memory, Span},
};
use support::{descriptor, Ram, BASE};

fn queue(ram: &Ram) -> queue::Queue {
  queue::create(ram, 8, BASE, BASE + 0x100, BASE + 0x200).unwrap()
}

#[test]
fn guest_buffers_transfer_across_fragment_boundaries() {
  let mut ram = Ram::default();
  let spans = [
    Span {
      address: BASE + 0x1000,
      length: 3,
    },
    Span {
      address: BASE + 0x2000,
      length: 5,
    },
  ];
  dma::write(&mut ram, &spans, 1, b"abcdef").unwrap();
  assert_eq!(&ram.0[0x1000..0x1003], b"\0ab");
  assert_eq!(&ram.0[0x2000..0x2005], b"cdef\0");
  let mut bytes = [0; 6];
  dma::read(&ram, &spans, 1, &mut bytes).unwrap();
  assert_eq!(&bytes, b"abcdef");
  assert!(dma::read(&ram, &spans, 3, &mut bytes).is_err());
  assert!(dma::read(&ram, &spans, u64::MAX, &mut bytes).is_err());
  assert!(dma::write(
    &mut ram,
    &[Span {
      address: u64::MAX,
      length: 2
    }],
    1,
    b"x"
  )
  .is_err());
  assert!(!ram.contains(BASE - 1, 1));
}

#[test]
fn requests_publish_used_lengths_and_honor_interrupt_suppression() {
  let mut ram = Ram::default();
  descriptor(&mut ram, 0, BASE + 0x1000, 16, 1, 1);
  descriptor(&mut ram, 1, BASE + 0x2000, 512, 3, 2);
  descriptor(&mut ram, 2, BASE + 0x3000, 1, 2, 0);
  ram.0[0x102..0x104].copy_from_slice(&1u16.to_le_bytes());
  let mut queue = queue(&ram);
  let chain = queue::pop(&mut queue, &mut ram).unwrap().unwrap();
  assert_eq!(chain.buffers.len(), 3);
  assert_eq!(chain.buffers[1].span.address, BASE + 0x2000);
  assert!(chain.buffers[1].writable);
  assert!(queue::pop(&mut queue, &mut ram).unwrap().is_none());
  assert!(queue::complete(&mut queue, &mut ram, &chain, 514).is_err());
  assert!(queue::complete(&mut queue, &mut ram, &chain, 513).unwrap());
  assert_eq!(&ram.0[0x202..0x204], &1u16.to_le_bytes());
  assert_eq!(&ram.0[0x204..0x20c], &[0, 0, 0, 0, 1, 2, 0, 0]);
  assert!(queue::complete(&mut queue, &mut ram, &chain, 513).is_err());
  ram.0[0x100] = 1;
  ram.0[0x102..0x104].copy_from_slice(&2u16.to_le_bytes());
  let chain = queue::pop(&mut queue, &mut ram).unwrap().unwrap();
  assert!(!queue::complete(&mut queue, &mut ram, &chain, 1).unwrap());
}

#[test]
fn malformed_guest_queues_and_chains_are_rejected() {
  let mut ram = Ram::default();
  assert!(queue::create(&ram, 3, BASE, BASE + 0x100, BASE + 0x200).is_err());
  assert!(queue::create(&ram, 8, BASE + 1, BASE + 0x100, BASE + 0x200).is_err());
  assert!(queue::create(&ram, 8, BASE, BASE + 0x10, BASE + 0x200).is_err());
  assert!(queue::create(&ram, 8, u64::MAX - 15, BASE + 0x100, BASE + 0x200).is_err());
  ram.0[0x102..0x104].copy_from_slice(&1u16.to_le_bytes());
  for (address, length, flags, next) in [
    (BASE + 0x1000, 16, 1, 0),
    (BASE + 0x1000, 16, 1, 8),
    (BASE + 0x1000, 16, 4, 0),
    (u64::MAX, 16, 0, 0),
    (BASE - 1, 16, 0, 0),
  ] {
    descriptor(&mut ram, 0, address, length, flags, next);
    assert!(queue::pop(&mut queue(&ram), &mut ram).is_err());
  }
  descriptor(&mut ram, 0, BASE + 0x1000, 16, 3, 1);
  descriptor(&mut ram, 1, BASE + 0x2000, 16, 0, 0);
  assert!(queue::pop(&mut queue(&ram), &mut ram).is_err());
  ram.0[0x102..0x104].copy_from_slice(&9u16.to_le_bytes());
  assert!(queue::pop(&mut queue(&ram), &mut ram).is_err());
}

#[test]
fn outstanding_heads_cannot_be_submitted_twice() {
  let mut ram = Ram::default();
  descriptor(&mut ram, 0, BASE + 0x1000, 1, 2, 0);
  ram.0[0x102..0x104].copy_from_slice(&2u16.to_le_bytes());
  let mut queue = queue(&ram);
  assert!(queue::pop(&mut queue, &mut ram).unwrap().is_some());
  assert!(queue::pop(&mut queue, &mut ram).is_err());
}

#[test]
fn sixteen_bit_ring_indices_wrap_without_losing_requests() {
  let mut ram = Ram::default();
  descriptor(&mut ram, 0, BASE + 0x1000, 1, 2, 0);
  let mut queue = queue(&ram);
  for index in 1..=65537u32 {
    ram.0[0x102..0x104].copy_from_slice(&(index as u16).to_le_bytes());
    let chain = queue::pop(&mut queue, &mut ram).unwrap().unwrap();
    queue::complete(&mut queue, &mut ram, &chain, 1).unwrap();
    assert_eq!(&ram.0[0x202..0x204], &(index as u16).to_le_bytes());
  }
}
