//! timer projection uses architectural wrapping counters, not wall-clock dates.

use crate::hypervisor::{check, ffi, Cpu};
use anyhow::ensure;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frozen {
  virtual_counter: u64,
}

impl Frozen {
  pub fn freeze(host: u64, virtual_offset: u64) -> Self {
    Self {
      virtual_counter: host.wrapping_sub(virtual_offset),
    }
  }

  pub fn rebase(&self, host: u64) -> u64 {
    host.wrapping_sub(self.virtual_counter)
  }

  pub(super) fn counter(&self) -> u64 {
    self.virtual_counter
  }
}

pub(crate) fn rate() -> anyhow::Result<(u32, u32)> {
  let mut info = ffi::Timebase {
    numerator: 0,
    denominator: 0,
  };
  ensure!(
    unsafe { ffi::mach_timebase_info(&mut info) } == 0
      && info.numerator != 0
      && info.denominator != 0,
    "Host timer units are unavailable"
  );
  let (mut a, mut b) = (info.numerator, info.denominator);
  while b != 0 {
    (a, b) = (b, a % b);
  }
  Ok((info.numerator / a, info.denominator / a))
}

pub fn ticks() -> u64 {
  unsafe { ffi::mach_absolute_time() }
}

/// Hold every CPU and use one host tick for the entire snapshot capture.
/// The CPU's timer registers must remain unchanged throughout that phase.
pub fn capture_at(cpu: &Cpu<'_>, host: u64) -> anyhow::Result<Frozen> {
  let mut offset = 0;
  check(
    unsafe { ffi::hv_vcpu_get_vtimer_offset(cpu.id, &mut offset) },
    "Read frozen CPU timer offset",
  )?;
  Ok(Frozen::freeze(host, offset))
}

/// Apply a shared resume epoch on the held CPU owner, after architectural restoration.
pub fn restore(cpu: &mut Cpu<'_>, clock: &Frozen, host: u64) -> anyhow::Result<()> {
  check(
    unsafe { ffi::hv_vcpu_set_vtimer_offset(cpu.id, clock.rebase(host)) },
    "Restore CPU resume epoch",
  )
}
