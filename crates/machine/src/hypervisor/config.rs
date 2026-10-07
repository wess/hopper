use super::{check, ffi};
use anyhow::Context;
use std::ptr::NonNull;

pub(super) fn create() -> anyhow::Result<()> {
  let config =
    NonNull::new(unsafe { ffi::hv_vm_config_create() }).context("Create VM configuration")?;
  let configured = check(
    unsafe { ffi::hv_vm_config_set_el2_enabled(config.as_ptr(), false) },
    "Disable nested guest virtualization",
  );
  let result =
    configured.and_then(|_| check(unsafe { ffi::hv_vm_create(config.as_ptr()) }, "Create VM"));
  unsafe { ffi::os_release(config.as_ptr()) };
  result
}

pub(super) fn cpu(id: u64, performance: bool) -> anyhow::Result<()> {
  check(
    unsafe { ffi::hv_vcpu_set_trap_debug_reg_accesses(id, false) },
    "Allow native guest debug registers",
  )?;
  if performance {
    let mut features = 0;
    check(
      unsafe { ffi::hv_vcpu_get_sys_reg(id, 0xc028, &mut features) },
      "Read performance features",
    )?;
    check(
      unsafe { ffi::hv_vcpu_set_sys_reg(id, 0xc028, (features & !0xf00) | 0x100) },
      "Enable virtual performance monitor",
    )?;
  }
  Ok(())
}
