//! ABI definitions from the Apple silicon Hypervisor SDK headers.

use std::ffi::c_void;

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct Exception {
  pub syndrome: u64,
  pub virtual_address: u64,
  pub physical_address: u64,
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct Exit {
  pub reason: u32,
  pub exception: Exception,
}

pub const PC: u32 = 31;
pub const CPSR: u32 = 34;

#[link(name = "Hypervisor", kind = "framework")]
extern "C" {
  pub fn hv_vm_create(config: *mut c_void) -> i32;
  pub fn hv_vm_destroy() -> i32;
  pub fn hv_vm_map(address: *mut c_void, guest: u64, size: usize, flags: u64) -> i32;
  pub fn hv_vm_unmap(guest: u64, size: usize) -> i32;
  pub fn hv_vm_protect(guest: u64, size: usize, flags: u64) -> i32;
  pub fn hv_vcpu_create(cpu: *mut u64, exit: *mut *mut Exit, config: *mut c_void) -> i32;
  pub fn hv_vcpu_destroy(cpu: u64) -> i32;
  pub fn hv_vcpu_run(cpu: u64) -> i32;
  pub fn hv_vcpus_exit(cpus: *mut u64, count: u32) -> i32;
  pub fn hv_vcpu_get_reg(cpu: u64, register: u32, value: *mut u64) -> i32;
  pub fn hv_vcpu_set_reg(cpu: u64, register: u32, value: u64) -> i32;
  pub fn hv_vcpu_set_sys_reg(cpu: u64, register: u16, value: u64) -> i32;
  pub fn hv_gic_config_create() -> *mut c_void;
  pub fn hv_gic_config_set_distributor_base(config: *mut c_void, address: u64) -> i32;
  pub fn hv_gic_config_set_redistributor_base(config: *mut c_void, address: u64) -> i32;
  pub fn hv_gic_create(config: *mut c_void) -> i32;
  pub fn hv_gic_get_distributor_size(size: *mut usize) -> i32;
  pub fn hv_gic_get_distributor_base_alignment(alignment: *mut usize) -> i32;
  pub fn hv_gic_get_redistributor_region_size(size: *mut usize) -> i32;
  pub fn hv_gic_get_redistributor_base_alignment(alignment: *mut usize) -> i32;
  pub fn hv_gic_get_spi_interrupt_range(base: *mut u32, count: *mut u32) -> i32;
  pub fn hv_gic_get_distributor_reg(register: u16, value: *mut u64) -> i32;
  pub fn hv_gic_set_spi(interrupt: u32, level: bool) -> i32;
}

extern "C" {
  pub fn os_release(object: *mut c_void);
}
