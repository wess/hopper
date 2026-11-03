use super::*;

#[test]
fn reopened_input_keeps_normalized_axes_held_keys_leds_and_pending_release_semantics() {
  let mut pointer = input::create(Kind::Tablet);
  input::enqueue(
    &mut pointer,
    &[
      Event {
        kind: 3,
        code: 0,
        value: 65535,
      },
      SYN,
    ],
  )
  .unwrap();
  let bytes = input::wire::encode(&pointer).unwrap();
  drop(pointer);
  let mut pointer = input::wire::decode(&bytes).unwrap();
  let mut ram = Ram::default();
  input::execute(&mut pointer, &mut ram, &chain(BASE, 8, true), 0).unwrap();
  assert_eq!(&ram.0[4..8], &32767i32.to_le_bytes());
  let mut keyboard = input::create(Kind::Keyboard);
  input::configure(&mut keyboard, 0, 2, 0x0111);
  input::enqueue(
    &mut keyboard,
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
  input::execute(&mut keyboard, &mut ram, &chain(BASE, 8, true), 0).unwrap();
  input::enqueue(
    &mut keyboard,
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
  ram.0[..8].copy_from_slice(&[17, 0, 1, 0, 1, 0, 0, 0]);
  input::execute(&mut keyboard, &mut ram, &chain(BASE, 8, false), 1).unwrap();
  let config = input::config(&keyboard);
  let bytes = input::wire::encode(&keyboard).unwrap();
  drop(keyboard);
  let mut keyboard = input::wire::decode(&bytes).unwrap();
  assert_eq!(input::config(&keyboard), config);
  assert_eq!(input::leds(&keyboard), 2);
  input::release(&mut keyboard);
  input::execute(&mut keyboard, &mut ram, &chain(BASE, 8, true), 0).unwrap();
  assert_eq!(&ram.0[..8], &[1, 0, 42, 0, 0, 0, 0, 0]);
  input::execute(&mut keyboard, &mut ram, &chain(BASE, 8, true), 0).unwrap();
  assert_eq!(&ram.0[..8], &[0; 8]);
  assert_eq!(input::pending(&keyboard), 0);
}

#[test]
fn input_records_reject_damage_and_rechecksummed_invalid_normalized_events() {
  use sha2::{Digest, Sha256};
  let mut pointer = input::create(Kind::Tablet);
  input::enqueue(
    &mut pointer,
    &[
      Event {
        kind: 3,
        code: 0,
        value: 65535,
      },
      SYN,
    ],
  )
  .unwrap();
  let bytes = input::wire::encode(&pointer).unwrap();
  for end in 0..bytes.len() {
    assert!(input::wire::decode(&bytes[..end]).is_err());
  }
  for offset in 0..bytes.len() {
    let mut changed = bytes.clone();
    changed[offset] ^= 1;
    assert!(input::wire::decode(&changed).is_err());
  }
  for (offset, value) in [
    (0, b'X'),
    (4, 3),
    (7, 1),
    (9, 255),
    (11, 1),
    (12, 2),
    (16, 1),
    (22, 1),
    (26, 1),
  ] {
    let mut changed = bytes.clone();
    changed[offset] = value;
    let end = changed.len() - 32;
    let checksum = Sha256::digest(&changed[..end]);
    changed[end..].copy_from_slice(&checksum);
    assert!(
      input::wire::decode(&changed).is_err(),
      "Accepted invalid input field at {offset}"
    );
  }
  let mut full = input::create(Kind::Keyboard);
  input::enqueue(&mut full, &vec![SYN; 1024]).unwrap();
  let bytes = input::wire::encode(&full).unwrap();
  assert_eq!(input::pending(&input::wire::decode(&bytes).unwrap()), 1024);
  assert!(input::wire::decode(&vec![0; input::wire::MAX + 1]).is_err());
}
