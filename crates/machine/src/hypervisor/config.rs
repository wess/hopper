use super::{check, ffi};
use anyhow::{ensure, Context};
use std::ptr::NonNull;

pub(super) fn create() -> anyhow::Result<u32> {
  let config =
    NonNull::new(unsafe { ffi::hv_vm_config_create() }).context("Create VM configuration")?;
  let result = (|| {
    let mut maximum = 0;
    check(
      unsafe { ffi::hv_vm_config_get_max_ipa_size(&mut maximum) },
      "Read VM address width",
    )?;
    ensure!(maximum >= 36, "Native VM address width is unsupported");
    // 64 GiB of RAM starts above zero; the SDK's default 36-bit range is too small.
    let bits = maximum.min(40);
    check(
      unsafe { ffi::hv_vm_config_set_ipa_size(config.as_ptr(), bits) },
      "Configure VM address width",
    )?;
    check(
      unsafe { ffi::hv_vm_config_set_el2_enabled(config.as_ptr(), false) },
      "Disable nested guest virtualization",
    )?;
    check(unsafe { ffi::hv_vm_create(config.as_ptr()) }, "Create VM")?;
    Ok(bits)
  })();
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
