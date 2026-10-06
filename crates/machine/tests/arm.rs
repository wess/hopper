use machine::arm::{decode, Access, Trap};

#[test]
fn firmware_call_immediates_are_preserved() {
  assert_eq!(decode((0x16 << 26) | 0x1234), Trap::Hypercall(0x1234));
  assert_eq!(decode(0x17 << 26), Trap::SecureCall(0));
}

#[test]
fn device_abort_contains_width_direction_and_register() {
  let syndrome =
    (0x24 << 26) | (1 << 24) | (3 << 22) | (1 << 21) | (7 << 16) | (1 << 15) | (1 << 6);
  assert_eq!(
    decode(syndrome),
    Trap::DataAbort(Some(Access {
      register: 7,
      bytes: 8,
      write: true,
      sign_extend: true,
      wide_register: true,
    }))
  );
}

#[test]
fn abort_without_instruction_syndrome_cannot_be_emulated() {
  assert_eq!(decode(0x24 << 26), Trap::DataAbort(None));
  assert_eq!(decode(0), Trap::Other(0));
}
