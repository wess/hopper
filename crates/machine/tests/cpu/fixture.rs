use crate::hypervisor::state::{debug, State, INTERRUPTS, SYSTEM};

pub(super) fn state() -> State {
  let mut features = [0; 12];
  features[5] = (1 << 8) | (15 << 12) | (15 << 20);
  let mut banks = vec![(0x8010, 0x1234), (0x8012, 0x5678)];
  for base in [0x8004, 0x8006] {
    for index in 0..16 {
      banks.extend([
        (base + index * 8, u64::MAX),
        (base + index * 8 + 1, 0x12345678),
      ]);
    }
  }
  State {
    features,
    debug: debug::Debug::from_values(features[5], banks).unwrap(),
    registers: [u64::MAX; 35],
    vectors: [[0xa5; 16]; 32],
    system: vec![0x1234567800000000; SYSTEM.len()],
    interrupts: vec![0x1234; INTERRUPTS.len()],
    timer_mask: true,
    timer_offset: u64::MAX,
    serror: true,
  }
}
