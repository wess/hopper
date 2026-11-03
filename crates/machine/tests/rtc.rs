use machine::devices::rtc::{self, wire};
use sha2::{Digest, Sha256};

#[test]
fn wall_clock_advances_and_guest_set_time_never_changes_its_host_source() {
  let mut clock = rtc::at(1_700_000_000);
  assert_eq!(
    rtc::read(&mut clock, 0, 1_700_000_123).unwrap(),
    1_700_000_123
  );
  rtc::write(&mut clock, 8, 1_800_000_000, 1_700_000_123).unwrap();
  assert_eq!(
    rtc::read(&mut clock, 8, 1_700_000_124).unwrap(),
    1_800_000_000
  );
  assert_eq!(
    rtc::read(&mut clock, 0, 1_700_000_124).unwrap(),
    1_800_000_001
  );
  let mut independent = rtc::at(1_700_000_124);
  assert_eq!(
    rtc::read(&mut independent, 0, 1_700_000_124).unwrap(),
    1_700_000_124
  );
  assert!(rtc::now().unwrap() > 1_700_000_000);
}

#[test]
fn firmware_identity_reads_and_reserved_registers_are_bounded() {
  let mut clock = rtc::at(100);
  for (offset, value) in [
    (0xfe0, 0x31),
    (0xfe4, 0x10),
    (0xfe8, 4),
    (0xfec, 0),
    (0xff0, 0x0d),
    (0xff4, 0xf0),
    (0xff8, 5),
    (0xffc, 0xb1),
  ] {
    assert_eq!(rtc::read(&mut clock, offset, 100).unwrap(), value);
    rtc::write(&mut clock, offset, 0, 100).unwrap();
    assert_eq!(rtc::read(&mut clock, offset, 100).unwrap(), value);
  }
  assert_eq!(rtc::read(&mut clock, 0x80, 100).unwrap(), 0);
  for offset in (0xfe0..0x1000).step_by(4) {
    assert!(rtc::accepts(offset, 1, false));
    assert!(!rtc::accepts(offset, 1, true));
    assert!(rtc::accepts(offset, 4, false));
  }
  assert!(!rtc::accepts(0, 1, false));
  assert!(!rtc::accepts(0, 8, false));
  assert!(!rtc::accepts(1, 4, false));
  assert!(!rtc::accepts(0x1000, 4, false));

  for offset in [1, 3, 0xfff, 0x1000, u64::MAX] {
    assert!(rtc::read(&mut clock, offset, 100).is_err());
    assert!(rtc::write(&mut clock, offset, 0, 100).is_err());
  }
}

#[test]
fn match_crossing_mask_clear_wrap_and_host_clock_reversal() {
  let mut clock = rtc::at(100);
  rtc::write(&mut clock, 4, 102, 100).unwrap();
  assert!(!rtc::interrupt(&mut clock, 103));
  assert_eq!(rtc::read(&mut clock, 20, 103).unwrap(), 1);
  rtc::write(&mut clock, 16, 1, 103).unwrap();
  assert!(rtc::interrupt(&mut clock, 103));
  rtc::write(&mut clock, 28, 0, 103).unwrap();
  assert!(rtc::interrupt(&mut clock, 103));
  rtc::write(&mut clock, 28, 1, 103).unwrap();
  assert!(!rtc::interrupt(&mut clock, 103));
  rtc::write(&mut clock, 4, 103, 103).unwrap();
  assert!(rtc::interrupt(&mut clock, 103));
  rtc::write(&mut clock, 28, 1, 103).unwrap();
  assert!(!rtc::interrupt(&mut clock, 103));
  rtc::write(&mut clock, 4, 105, 103).unwrap();
  assert!(!rtc::interrupt(&mut clock, 100));
  assert!(rtc::interrupt(&mut clock, 106));
  let mut wrap = rtc::at(u32::MAX - 1);
  rtc::write(&mut wrap, 4, 0, u32::MAX - 1).unwrap();
  rtc::write(&mut wrap, 16, 1, u32::MAX - 1).unwrap();
  assert!(rtc::interrupt(&mut wrap, 1));
}

#[test]
fn control_reset_and_disabled_counter_survive_record_reopening() {
  let mut clock = rtc::at(100);
  rtc::write(&mut clock, 12, 0, 101).unwrap();
  assert_eq!(rtc::read(&mut clock, 0, 200).unwrap(), 0);
  rtc::write(&mut clock, 8, 500, 200).unwrap();
  let bytes = wire::encode(&clock);
  let mut reopened = wire::decode(&bytes).unwrap();
  assert_eq!(rtc::read(&mut reopened, 0, 1000).unwrap(), 500);
  rtc::write(&mut reopened, 12, 1, 1000).unwrap();
  assert_eq!(rtc::read(&mut reopened, 0, 1001).unwrap(), 501);
  rtc::write(&mut reopened, 12, 1, 1001).unwrap();
  assert_eq!(rtc::read(&mut reopened, 0, 1002).unwrap(), 1);
}

#[test]
fn enabled_snapshot_preserves_offset_alarm_and_full_record_integrity() {
  let mut clock = rtc::at(100);
  rtc::write(&mut clock, 8, 500, 100).unwrap();
  rtc::write(&mut clock, 4, 502, 100).unwrap();
  rtc::write(&mut clock, 16, 1, 100).unwrap();
  let bytes = wire::encode(&clock);
  let mut reopened = wire::decode(&bytes).unwrap();
  assert_eq!(rtc::read(&mut reopened, 0, 103).unwrap(), 503);
  assert!(rtc::interrupt(&mut reopened, 103));
  for end in 0..bytes.len() {
    assert!(wire::decode(&bytes[..end]).is_err());
  }
  for index in 0..bytes.len() {
    let mut damaged = bytes;
    damaged[index] ^= 1;
    assert!(wire::decode(&damaged).is_err());
  }
  for (range, value) in [(4..12, i64::MAX.to_le_bytes().to_vec()), (28..29, vec![8])] {
    let mut damaged = bytes;
    damaged[range].copy_from_slice(&value);
    let hash = Sha256::digest(&damaged[..29]);
    damaged[29..].copy_from_slice(&hash);
    assert!(wire::decode(&damaged).is_err());
  }
}
