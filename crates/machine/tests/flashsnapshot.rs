use machine::devices::flash::{self, snapshot};

fn bank() -> flash::Flash {
  flash::create(vec![0xff; 0x10000], 0x1000).unwrap()
}

#[test]
fn every_command_boundary_reconstructs_and_continues_like_the_original_bank() {
  let traces: &[&[(usize, u32)]] = &[
    &[
      (0, 0x40),
      (0, 0x12345678),
      (0, 0xff),
      (0, 0x20),
      (0, 0xd0),
      (0, 0xff),
    ],
    &[
      (0, 0x60),
      (0, 1),
      (0, 0x40),
      (0, 0),
      (0, 0x90),
      (0, 0xff),
      (0, 0x60),
      (0, 0xd0),
    ],
    &[
      (128, 0xe8),
      (128, 1),
      (128, 0x12345678),
      (132, 0xabcdef01),
      (128, 0xd0),
      (0, 0xff),
    ],
    &[
      (0, 0x98),
      (0, 0xff),
      (0, 0x90),
      (0, 0xff),
      (0, 0x70),
      (0, 0x50),
    ],
  ];
  for trace in traces {
    for boundary in 0..=trace.len() {
      let mut original = bank();
      for &(offset, value) in &trace[..boundary] {
        flash::write(&mut original, offset, value).unwrap();
      }
      let saved = reopen(snapshot::capture(&original));
      let mut restored = snapshot::restore(&saved, flash::bytes(&original).to_vec()).unwrap();
      compare(&original, &restored);
      for &(offset, value) in &trace[boundary..] {
        assert_eq!(
          flash::write(&mut original, offset, value).unwrap(),
          flash::write(&mut restored, offset, value).unwrap()
        );
        compare(&original, &restored);
      }
    }
  }
}

#[test]
fn captured_buffer_is_independent_and_reusable() {
  let mut original = bank();
  flash::write(&mut original, 0, 0xe8).unwrap();
  flash::write(&mut original, 0, 1).unwrap();
  flash::write(&mut original, 0, 0x12345678).unwrap();
  let saved = reopen(snapshot::capture(&original));
  let bytes = flash::bytes(&original).to_vec();
  flash::write(&mut original, 4, 0).unwrap();
  flash::write(&mut original, 0, 0xd0).unwrap();
  for _ in 0..2 {
    let mut restored = snapshot::restore(&saved, bytes.clone()).unwrap();
    flash::write(&mut restored, 4, 0xabcdef01).unwrap();
    assert_eq!(flash::write(&mut restored, 0, 0xd0).unwrap(), Some(0..8));
    flash::write(&mut restored, 0, 0xff).unwrap();
    assert_eq!(flash::read(&restored, 0, 8).unwrap(), 0xabcdef0112345678);
  }
  assert_eq!(&flash::bytes(&original)[4..8], &[0; 4]);
  assert!(snapshot::restore(&saved, vec![0xff; 0x20000]).is_err());
  assert!(snapshot::restore(&saved, vec![0xff; 0x8000]).is_err());
}

fn compare(first: &flash::Flash, second: &flash::Flash) {
  assert_eq!(flash::bytes(first), flash::bytes(second));
  assert_eq!(flash::array(first), flash::array(second));
  for offset in [0, 4, 8, 0x40, 0x9c, 128, 132, 0x1000, 0x1008] {
    assert_eq!(
      flash::read(first, offset, 4).unwrap(),
      flash::read(second, offset, 4).unwrap()
    );
  }
}

fn reopen(saved: snapshot::State) -> snapshot::State {
  let bytes = snapshot::wire::encode(&saved).unwrap();
  drop(saved);
  snapshot::wire::decode(&bytes).unwrap()
}
