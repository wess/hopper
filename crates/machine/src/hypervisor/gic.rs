//! The kernel-backed GIC handles interrupt routing and guest register accesses.

use super::{check, ffi, Vm};
use anyhow::{ensure, Context};
use std::ffi::c_void;
use std::ptr::NonNull;

pub struct Gic<'a> {
  pub distributor: u64,
  pub distributor_size: usize,
  pub redistributor: u64,
  pub redistributor_size: usize,
  pub spi_base: u32,
  pub spi_count: u32,
  _vm: &'a Vm,
}

struct Config(NonNull<c_void>);

impl Drop for Config {
  fn drop(&mut self) {
    unsafe { ffi::os_release(self.0.as_ptr()) };
  }
}

pub fn create(vm: &Vm, distributor: u64, redistributor: u64) -> anyhow::Result<Gic<'_>> {
  let mut distributor_size = 0;
  let mut distributor_alignment = 0;
  let mut redistributor_size = 0;
  let mut redistributor_alignment = 0;
  let mut spi_base = 0;
  let mut spi_count = 0;
  unsafe {
    check(
      ffi::hv_gic_get_distributor_size(&mut distributor_size),
      "Read GIC distributor size",
    )?;
    check(
      ffi::hv_gic_get_distributor_base_alignment(&mut distributor_alignment),
      "Read GIC distributor alignment",
    )?;
    check(
      ffi::hv_gic_get_redistributor_region_size(&mut redistributor_size),
      "Read GIC redistributor size",
    )?;
    check(
      ffi::hv_gic_get_redistributor_base_alignment(&mut redistributor_alignment),
      "Read GIC redistributor alignment",
    )?;
    check(
      ffi::hv_gic_get_spi_interrupt_range(&mut spi_base, &mut spi_count),
      "Read GIC interrupt range",
    )?;
  }
  ensure!(
    distributor_alignment > 0 && distributor.is_multiple_of(distributor_alignment as u64),
    "GIC distributor address is not aligned"
  );
  ensure!(
    redistributor_alignment > 0 && redistributor.is_multiple_of(redistributor_alignment as u64),
    "GIC redistributor address is not aligned"
  );
  let distributor_end = distributor
    .checked_add(distributor_size as u64)
    .context("GIC distributor range overflow")?;
  let redistributor_end = redistributor
    .checked_add(redistributor_size as u64)
    .context("GIC redistributor range overflow")?;
  ensure!(
    distributor_end <= redistributor || redistributor_end <= distributor,
    "GIC regions overlap"
  );
  let config = Config(
    NonNull::new(unsafe { ffi::hv_gic_config_create() }).context("Create GIC configuration")?,
  );
  unsafe {
    check(
      ffi::hv_gic_config_set_distributor_base(config.0.as_ptr(), distributor),
      "Set GIC distributor address",
    )?;
    check(
      ffi::hv_gic_config_set_redistributor_base(config.0.as_ptr(), redistributor),
      "Set GIC redistributor address",
    )?;
    check(
      ffi::hv_gic_create(config.0.as_ptr()),
      "Create interrupt controller",
    )?;
  }
  vm.gic.set(true);
  Ok(Gic {
    distributor,
    distributor_size,
    redistributor,
    redistributor_size,
    spi_base,
    spi_count,
    _vm: vm,
  })
}

pub fn signal(gic: &Gic<'_>, interrupt: u32, level: bool) -> anyhow::Result<()> {
  ensure!(
    interrupt >= gic.spi_base && interrupt - gic.spi_base < gic.spi_count,
    "Peripheral interrupt is outside the GIC range"
  );
  check(
    unsafe { ffi::hv_gic_set_spi(interrupt, level) },
    "Signal peripheral interrupt",
  )
}

pub fn read(_gic: &Gic<'_>, offset: u16) -> anyhow::Result<u64> {
  let mut value = 0;
  check(
    unsafe { ffi::hv_gic_get_distributor_reg(offset, &mut value) },
    "Read GIC distributor",
  )?;
  Ok(value)
}
