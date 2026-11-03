use super::super::{check, ffi, state, Cpu};
use anyhow::Context;
use std::time::Duration;

pub(super) fn run<T>(
  cpu: &mut Cpu<'_>,
  vectors: u64,
  work: impl FnOnce(&mut Cpu<'_>) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
  let architectural = state::capture(cpu)?;
  let mut trap_debug = false;
  check(
    unsafe { ffi::hv_vcpu_get_trap_debug_exceptions(cpu.id, &mut trap_debug) },
    "Read PMU context debug trap",
  )?;
  let result = (|| {
    check(
      unsafe { ffi::hv_vcpu_set_vtimer_mask(cpu.id, true) },
      "Mask PMU context timer",
    )?;
    check(
      unsafe { ffi::hv_vcpu_set_serror(cpu.id, false) },
      "Hold PMU context SError",
    )?;
    check(
      unsafe { ffi::hv_vcpu_set_trap_debug_exceptions(cpu.id, true) },
      "Trap PMU context debug exceptions",
    )?;
    write(cpu, 0x8012, 0)?;
    write(cpu, 0xc600, vectors)?;
    write(cpu, 0xc080, 0)?;
    super::super::bounded(cpu, Duration::from_millis(100), work)
  })();
  // first guest entry can change additional SDK-managed state, even on rejection.
  let restored = state::restore(cpu, &architectural);
  let debug = check(
    unsafe { ffi::hv_vcpu_set_trap_debug_exceptions(cpu.id, trap_debug) },
    "Restore PMU context debug trap",
  );
  restored.context("Restore architectural PMU context; discard VM on failure")?;
  debug.context("Restore PMU debug configuration; discard VM on failure")?;
  result
}

fn write(cpu: &Cpu<'_>, register: u16, value: u64) -> anyhow::Result<()> {
  check(
    unsafe { ffi::hv_vcpu_set_sys_reg(cpu.id, register, value) },
    "Write PMU context system register",
  )
}
