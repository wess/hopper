//! PSCI services; stateless calls describe a single CPU.

pub mod power;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reply {
  Value(i64),
  CpuOn {
    target: usize,
    entry: u64,
    context: u64,
  },
  CpuOff,
  Shutdown,
  Reset,
}

fn supported(command: u32) -> bool {
  matches!(
    command,
    0x84000000 | 0x84000002 | 0x84000004 | 0xc4000004 | 0x84000008 | 0x84000009 | 0x8400000a
  )
}

pub fn call(command: u32, argument: u64, level: u32) -> Reply {
  match command {
    0x84000000 => Reply::Value(0x10000),
    0x84000002 => Reply::CpuOff,
    0x84000004 | 0xc4000004 => Reply::Value(if argument & 0xff00ffffff == 0 && level <= 3 {
      0
    } else {
      -2
    }),
    0x84000008 => Reply::Shutdown,
    0x84000009 => Reply::Reset,
    0x8400000a => Reply::Value(if supported(argument as u32) { 0 } else { -1 }),
    _ => Reply::Value(-1),
  }
}
