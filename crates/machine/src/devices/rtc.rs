//! PL031 wall clock, independent of paused CPU timer epochs and scoped to this guest.

use anyhow::ensure;
use std::time::{SystemTime, UNIX_EPOCH};
pub mod wire;

#[derive(Clone)]
pub struct Clock {
  offset: i64,
  halted: u32,
  loaded: u32,
  matched: u32,
  last: u32,
  enabled: bool,
  mask: bool,
  pending: bool,
}

pub fn now() -> anyhow::Result<u32> {
  let seconds = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
  ensure!(
    seconds <= u32::MAX as u64,
    "Host time exceeds the PL031 epoch range"
  );
  Ok(seconds as u32)
}

pub fn create() -> anyhow::Result<Clock> {
  Ok(at(now()?))
}

pub fn at(host: u32) -> Clock {
  Clock {
    offset: 0,
    halted: host,
    loaded: 0,
    matched: 0,
    last: host,
    enabled: true,
    mask: false,
    pending: false,
  }
}

fn counter(clock: &Clock, host: u32) -> u32 {
  if clock.enabled {
    (i64::from(host) + clock.offset) as u32
  } else {
    clock.halted
  }
}

fn update(clock: &mut Clock, host: u32) -> u32 {
  let value = counter(clock, host);
  let elapsed = value.wrapping_sub(clock.last);
  let until = clock.matched.wrapping_sub(clock.last);
  if elapsed != 0 && elapsed <= i32::MAX as u32 && until != 0 && until <= elapsed {
    clock.pending = true;
  }
  clock.last = value;
  value
}

pub fn interrupt(clock: &mut Clock, host: u32) -> bool {
  update(clock, host);
  clock.pending && clock.mask
}

pub fn accepts(offset: u64, bytes: u8, write: bool) -> bool {
  offset < 0x1000
    && offset.is_multiple_of(4)
    && (bytes == 4 || (bytes == 1 && !write && offset >= 0xfe0))
}

pub fn read(clock: &mut Clock, offset: u64, host: u32) -> anyhow::Result<u32> {
  ensure!(
    offset < 0x1000 && offset.is_multiple_of(4),
    "Invalid RTC register access"
  );
  let value = update(clock, host);
  Ok(match offset {
    0 => value,
    4 => clock.matched,
    8 => clock.loaded,
    12 => clock.enabled as u32,
    16 => clock.mask as u32,
    20 => clock.pending as u32,
    24 => (clock.pending && clock.mask) as u32,
    0xfe0 => 0x31,
    0xfe4 => 0x10,
    0xfe8 => 0x04,
    0xfec => 0,
    0xff0 => 0x0d,
    0xff4 => 0xf0,
    0xff8 => 0x05,
    0xffc => 0xb1,
    _ => 0,
  })
}

pub fn write(clock: &mut Clock, offset: u64, value: u32, host: u32) -> anyhow::Result<()> {
  ensure!(
    offset < 0x1000 && offset.is_multiple_of(4),
    "Invalid RTC register access"
  );
  let current = update(clock, host);
  match offset {
    4 => {
      clock.matched = value;
      clock.pending |= value == current;
    }
    8 => {
      clock.loaded = value;
      clock.halted = value;
      clock.offset = i64::from(value) - i64::from(host);
      clock.last = value;
      clock.pending |= value == clock.matched;
    }
    12 => {
      // rewriting an enabled PL031 start bit resets its counter.
      if clock.enabled {
        clock.halted = 0;
      }
      clock.enabled = value & 1 != 0;
      clock.offset = i64::from(clock.halted) - i64::from(host);
      clock.last = clock.halted;
      clock.pending |= clock.halted == clock.matched;
    }
    16 => clock.mask = value & 1 != 0,
    28 if value & 1 != 0 => clock.pending = false,
    _ => {}
  }
  Ok(())
}
