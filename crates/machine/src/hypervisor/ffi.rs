//! ABI definitions from the Apple silicon Hypervisor SDK headers.

use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::OnceLock;

const HV_UNSUPPORTED: i32 = 0xfae9400fu32 as i32;

type GetSerror = unsafe extern "C" fn(u64, *mut bool) -> i32;
type SetSerror = unsafe extern "C" fn(u64, bool) -> i32;

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

#[repr(C)]
pub struct Timebase {
  pub numerator: u32,
  pub denominator: u32,
}

#[link(name = "Hypervisor", kind = "framework")]
extern "C" {
  pub fn hv_vm_create(config: *mut c_void) -> i32;
  pub fn hv_vm_config_create() -> *mut c_void;
  pub fn hv_vm_config_get_max_ipa_size(bits: *mut u32) -> i32;
  pub fn hv_vm_config_set_ipa_size(config: *mut c_void, bits: u32) -> i32;
  pub fn hv_vm_config_set_el2_enabled(config: *mut c_void, value: bool) -> i32;
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
  pub fn hv_vcpu_get_sys_reg(cpu: u64, register: u16, value: *mut u64) -> i32;
  pub fn hv_vcpu_get_vtimer_mask(cpu: u64, value: *mut bool) -> i32;
  pub fn hv_vcpu_set_vtimer_mask(cpu: u64, value: bool) -> i32;
  pub fn hv_vcpu_get_vtimer_offset(cpu: u64, value: *mut u64) -> i32;
  pub fn hv_vcpu_set_vtimer_offset(cpu: u64, value: u64) -> i32;
  pub fn hv_vcpu_set_trap_debug_reg_accesses(cpu: u64, value: bool) -> i32;
  pub fn hv_vcpu_get_trap_debug_exceptions(cpu: u64, value: *mut bool) -> i32;
  pub fn hv_vcpu_set_trap_debug_exceptions(cpu: u64, value: bool) -> i32;
  pub fn hv_gic_config_create() -> *mut c_void;
  pub fn hv_gic_config_set_distributor_base(config: *mut c_void, address: u64) -> i32;
  pub fn hv_gic_config_set_redistributor_base(config: *mut c_void, address: u64) -> i32;
  pub fn hv_gic_config_set_msi_region_base(config: *mut c_void, address: u64) -> i32;
  pub fn hv_gic_config_set_msi_interrupt_range(config: *mut c_void, base: u32, count: u32) -> i32;
  pub fn hv_gic_create(config: *mut c_void) -> i32;
  pub fn hv_gic_get_distributor_size(size: *mut usize) -> i32;
  pub fn hv_gic_get_distributor_base_alignment(alignment: *mut usize) -> i32;
  pub fn hv_gic_get_redistributor_region_size(size: *mut usize) -> i32;
  pub fn hv_gic_get_redistributor_base_alignment(alignment: *mut usize) -> i32;
  pub fn hv_gic_get_spi_interrupt_range(base: *mut u32, count: *mut u32) -> i32;
  pub fn hv_gic_get_msi_region_size(size: *mut usize) -> i32;
  pub fn hv_gic_get_msi_region_base_alignment(alignment: *mut usize) -> i32;
  pub fn hv_gic_get_msi_reg(register: u16, value: *mut u64) -> i32;
  pub fn hv_gic_send_msi(address: u64, interrupt: u32) -> i32;
  pub fn hv_gic_get_distributor_reg(register: u16, value: *mut u64) -> i32;
  pub fn hv_gic_set_spi(interrupt: u32, level: bool) -> i32;
  pub fn hv_gic_state_create() -> *mut c_void;
  pub fn hv_gic_state_get_size(state: *mut c_void, size: *mut usize) -> i32;
  pub fn hv_gic_state_get_data(state: *mut c_void, data: *mut c_void) -> i32;
  pub fn hv_gic_set_state(data: *const c_void, size: usize) -> i32;
  pub fn hv_gic_get_icc_reg(cpu: u64, register: u16, value: *mut u64) -> i32;
  pub fn hv_gic_set_icc_reg(cpu: u64, register: u16, value: u64) -> i32;
}

pub unsafe fn hv_vcpu_get_serror(cpu: u64, value: *mut bool) -> i32 {
  static FUNCTION: OnceLock<Option<GetSerror>> = OnceLock::new();
  match *FUNCTION.get_or_init(|| symbol(c"hv_vcpu_get_serror").map(|address| unsafe {
    std::mem::transmute::<*mut c_void, GetSerror>(address)
  })) {
    Some(function) => unsafe { function(cpu, value) },
    None => HV_UNSUPPORTED,
  }
}

pub unsafe fn hv_vcpu_set_serror(cpu: u64, value: bool) -> i32 {
  static FUNCTION: OnceLock<Option<SetSerror>> = OnceLock::new();
  match *FUNCTION.get_or_init(|| symbol(c"hv_vcpu_set_serror").map(|address| unsafe {
    std::mem::transmute::<*mut c_void, SetSerror>(address)
  })) {
    Some(function) => unsafe { function(cpu, value) },
    None => HV_UNSUPPORTED,
  }
}

fn symbol(name: &std::ffi::CStr) -> Option<*mut c_void> {
  NonNull::new(unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr()) }).map(NonNull::as_ptr)
}

extern "C" {
  pub fn os_release(object: *mut c_void);
  pub fn mach_absolute_time() -> u64;
  pub fn mach_timebase_info(info: *mut Timebase) -> i32;
  pub fn hopper_simd_get(cpu: u64, register: u32, bytes: *mut u8) -> i32;
  pub fn hopper_simd_set(cpu: u64, register: u32, bytes: *const u8) -> i32;
}
