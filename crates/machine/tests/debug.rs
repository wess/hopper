use machine::{arm::SystemAccess, debug};

fn access(encoding: u16, read: bool) -> SystemAccess {
  SystemAccess {
    encoding,
    register: 0,
    read,
  }
}

#[test]
fn guest_os_lock_resets_locked_and_honors_only_its_lock_bit() {
  let mut state = debug::State::default();
  let status = access(0x808c, true);
  let write = access(0x8084, false);
  assert_eq!(debug::access(&mut state, status, 0), Some(10));
  assert_eq!(debug::access(&mut state, write, !1), Some(0));
  assert_eq!(debug::access(&mut state, status, 0), Some(8));
  assert_eq!(debug::access(&mut state, write, 1), Some(0));
  assert_eq!(debug::access(&mut state, status, 0), Some(10));
  assert_eq!(
    debug::access(&mut debug::State::default(), status, 0),
    Some(10)
  );
}

#[test]
fn unsupported_registers_and_invalid_directions_are_not_emulated() {
  let mut state = debug::State::default();
  for request in [
    access(0x808c, false),
    access(0x8084, true),
    access(0xdce0, true),
  ] {
    assert_eq!(debug::access(&mut state, request, 0), None);
  }
  assert_eq!(debug::access(&mut state, access(0x808c, true), 0), Some(10));
}
