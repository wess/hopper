use crate::arm::SystemAccess;

pub struct State {
  locked: bool,
}

impl Default for State {
  fn default() -> Self {
    Self { locked: true }
  }
}

pub fn access(state: &mut State, access: SystemAccess, value: u64) -> Option<u64> {
  match (access.encoding, access.read) {
    // armv8 OS lock model 2; the access register is write-only.
    (0x808c, true) => Some(8 | (state.locked as u64) << 1),
    (0x8084, false) => {
      state.locked = value & 1 != 0;
      Some(0)
    }
    _ => None,
  }
}
