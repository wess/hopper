use machine::devices::flash::{self, Flash};

fn bank() -> Flash {
  flash::create(vec![0xff; 0x80000], 0x10000).unwrap()
}

#[test]
fn firmware_can_program_erase_and_read_a_variable_word() {
  let mut bank = bank();
  flash::write(&mut bank, 0, 0x00400040).unwrap();
  assert_eq!(flash::write(&mut bank, 0, 0x12345678).unwrap(), Some(0..4));
  flash::write(&mut bank, 0, 0x00700070).unwrap();
  assert_eq!(flash::read(&bank, 0, 4).unwrap(), 0x00800080);
  flash::write(&mut bank, 0, 0x00ff00ff).unwrap();
  assert_eq!(flash::read(&bank, 0, 4).unwrap(), 0x12345678);
  flash::write(&mut bank, 0, 0x00200020).unwrap();
  assert_eq!(
    flash::write(&mut bank, 0, 0x00d000d0).unwrap(),
    Some(0..0x10000)
  );
  flash::write(&mut bank, 0, 0x00ff00ff).unwrap();
  assert_eq!(flash::read(&bank, 0, 4).unwrap(), 0xffffffff);
}

#[test]
fn program_cannot_set_cleared_bits_without_erasing() {
  let mut bank = bank();
  flash::write(&mut bank, 0, 0x00400040).unwrap();
  flash::write(&mut bank, 0, 0).unwrap();
  flash::write(&mut bank, 0, 0x00400040).unwrap();
  assert_eq!(flash::write(&mut bank, 0, u32::MAX).unwrap(), None);
  assert_eq!(flash::read(&bank, 0, 4).unwrap(), 0x00900090);
  flash::write(&mut bank, 0, 0xff).unwrap();
  assert_eq!(flash::read(&bank, 0, 4).unwrap(), 0);
}

#[test]
fn buffered_program_is_committed_only_after_confirmation() {
  let mut bank = bank();
  flash::write(&mut bank, 0, 0x00e800e8).unwrap();
  flash::write(&mut bank, 0, 0x00010001).unwrap();
  flash::write(&mut bank, 0, 0x12345678).unwrap();
  flash::write(&mut bank, 4, 0xabcdef01).unwrap();
  assert_eq!(&flash::bytes(&bank)[..8], &[0xff; 8]);
  assert_eq!(flash::write(&mut bank, 0, 0xd0).unwrap(), Some(0..8));
  flash::write(&mut bank, 0, 0xff).unwrap();
  assert_eq!(flash::read(&bank, 0, 8).unwrap(), 0xabcdef0112345678);
}

#[test]
fn locked_blocks_reject_program_and_erase_until_unlocked() {
  let mut bank = bank();
  flash::write(&mut bank, 0, 0x60).unwrap();
  flash::write(&mut bank, 0, 1).unwrap();
  flash::write(&mut bank, 0, 0x40).unwrap();
  assert_eq!(flash::write(&mut bank, 0, 0).unwrap(), None);
  flash::write(&mut bank, 0, 0x20).unwrap();
  assert_eq!(flash::write(&mut bank, 0, 0xd0).unwrap(), None);
  flash::write(&mut bank, 0, 0x60).unwrap();
  flash::write(&mut bank, 0, 0xd0).unwrap();
  flash::write(&mut bank, 0, 0x50).unwrap();
  flash::write(&mut bank, 0, 0x40).unwrap();
  assert_eq!(flash::write(&mut bank, 0, 0).unwrap(), Some(0..4));
}

#[test]
fn query_mode_reports_interleaved_geometry_without_changing_data() {
  let mut bank = bank();
  flash::write(&mut bank, 0x40, 0x00980098).unwrap();
  assert_eq!(flash::read(&bank, 0x40, 4).unwrap(), 0x00510051);
  assert_eq!(flash::read(&bank, 0x9c, 4).unwrap(), 0x00120012);
  flash::write(&mut bank, 0, 0xff).unwrap();
  assert!(flash::bytes(&bank).iter().all(|byte| *byte == 0xff));
  assert!(flash::read(&bank, 0x7ffff, 4).is_err());
  assert!(flash::write(&mut bank, 1, 0).is_err());
}
