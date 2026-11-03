mod support;
use machine::devices::virtio::queue::{self, snapshot};
use support::{descriptor, Ram, BASE};

fn queue(ram: &Ram) -> queue::Queue {
  queue::create(ram, 8, BASE, BASE + 0x100, BASE + 0x200).unwrap()
}

#[test]
fn capture_rejects_unfinished_requests_and_restore_never_writes_ram() {
  let mut ram = Ram::default();
  descriptor(&mut ram, 0, BASE + 0x1000, 8, 2, 0);
  ram.0[0x102..0x104].copy_from_slice(&1u16.to_le_bytes());
  let mut queue = queue(&ram);
  let chain = queue::pop(&mut queue, &mut ram).unwrap().unwrap();
  assert!(snapshot::capture(&queue).is_err());
  queue::complete(&mut queue, &mut ram, &chain, 8).unwrap();
  let saved = reopen(snapshot::capture(&queue).unwrap());
  let original = ram.0.clone();
  let mut restored = snapshot::restore(&saved, &ram).unwrap();
  assert_eq!(ram.0, original);
  assert!(queue::pop(&mut restored, &mut ram).unwrap().is_none());
  ram.0[0x202] = 2;
  let invalid = ram.0.clone();
  assert!(snapshot::restore(&saved, &ram).is_err());
  assert_eq!(ram.0, invalid);
  assert!(snapshot::restore(&saved, &Ram(vec![0; 256])).is_err());
  ram.0 = original;
  ram.0[0x102..0x104].copy_from_slice(&10u16.to_le_bytes());
  assert!(snapshot::restore(&saved, &ram).is_err());
}

#[test]
fn restored_indices_wrap_without_replaying_completed_requests() {
  let mut ram = Ram::default();
  descriptor(&mut ram, 0, BASE + 0x1000, 8, 2, 0);
  let mut original = queue(&ram);
  for index in 1..=65535u16 {
    ram.0[0x102..0x104].copy_from_slice(&index.to_le_bytes());
    let chain = queue::pop(&mut original, &mut ram).unwrap().unwrap();
    queue::complete(&mut original, &mut ram, &chain, 8).unwrap();
  }
  let saved = reopen(snapshot::capture(&original).unwrap());
  let bytes = ram.0.clone();
  drop(original);
  for _ in 0..2 {
    let mut ram = Ram(bytes.clone());
    let mut restored = snapshot::restore(&saved, &ram).unwrap();
    assert!(queue::pop(&mut restored, &mut ram).unwrap().is_none());
    for index in [0u16, 1] {
      ram.0[0x102..0x104].copy_from_slice(&index.to_le_bytes());
      let chain = queue::pop(&mut restored, &mut ram).unwrap().unwrap();
      queue::complete(&mut restored, &mut ram, &chain, 8).unwrap();
      assert_eq!(&ram.0[0x202..0x204], &index.to_le_bytes());
      assert!(queue::pop(&mut restored, &mut ram).unwrap().is_none());
    }
  }
}

#[test]
fn uninitialized_capture_defers_used_ring_initialization_until_pop() {
  let mut ram = Ram::default();
  ram.0[0x200..0x204].fill(0xa5);
  let saved = reopen(snapshot::capture(&queue(&ram)).unwrap());
  let mut restored = snapshot::restore(&saved, &ram).unwrap();
  assert_eq!(&ram.0[0x200..0x204], &[0xa5; 4]);
  assert!(queue::pop(&mut restored, &mut ram).unwrap().is_none());
  assert_eq!(&ram.0[0x200..0x204], &[0; 4]);
}

fn reopen(saved: snapshot::State) -> snapshot::State {
  let bytes = snapshot::wire::encode(&saved).unwrap();
  snapshot::wire::decode(&bytes).unwrap()
}
