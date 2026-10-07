use super::{check, config, ffi, Exit, Vm};
use anyhow::{bail, ensure};
use std::{
  marker::PhantomData,
  ptr::{null_mut, NonNull},
};

/// A CPU cannot move to another owner thread.
/// ```compile_fail
/// let vm = machine::hypervisor::create().unwrap();
/// let cpu = machine::hypervisor::cpu(&vm).unwrap();
/// std::thread::scope(|scope| { scope.spawn(move || drop(cpu)); });
/// ```
pub struct Cpu<'a> {
  pub(super) id: u64,
  exit: NonNull<ffi::Exit>,
  _vm: PhantomData<&'a Vm>,
}

/// A scoped creation token that can cross threads; each created CPU stays on its owner.
/// ```compile_fail
/// let token = {
///   let vm = machine::hypervisor::create().unwrap();
///   machine::hypervisor::factory(&vm)
/// };
/// std::thread::scope(|scope| {
///   scope.spawn(move || { let _ = machine::hypervisor::create_cpu(token); });
/// });
/// ```
#[derive(Clone, Copy)]
pub struct CpuFactory<'vm> {
  performance: bool,
  _vm: PhantomData<&'vm ()>,
}

pub fn factory(vm: &Vm) -> CpuFactory<'_> {
  CpuFactory {
    performance: vm.gic.get(),
    _vm: PhantomData,
  }
}

pub fn cpu(vm: &Vm) -> anyhow::Result<Cpu<'_>> {
  create_cpu(factory(vm))
}

pub fn create_cpu(factory: CpuFactory<'_>) -> anyhow::Result<Cpu<'_>> {
  let mut id = 0;
  let mut exit = null_mut();
  check(
    unsafe { ffi::hv_vcpu_create(&mut id, &mut exit, null_mut()) },
    "Create CPU",
  )?;
  let Some(exit) = NonNull::new(exit) else {
    unsafe { ffi::hv_vcpu_destroy(id) };
    bail!("Hypervisor did not provide CPU exit information");
  };
  let cpu = Cpu {
    id,
    exit,
    _vm: PhantomData,
  };
  config::cpu(cpu.id, factory.performance)?;
  Ok(cpu)
}

pub fn set(cpu: &mut Cpu<'_>, register: u32, value: u64) -> anyhow::Result<()> {
  ensure!(register <= ffi::CPSR, "Invalid CPU register");
  check(
    unsafe { ffi::hv_vcpu_set_reg(cpu.id, register, value) },
    "Set CPU register",
  )
}

pub fn get(cpu: &Cpu<'_>, register: u32) -> anyhow::Result<u64> {
  ensure!(register <= ffi::CPSR, "Invalid CPU register");
  let mut value = 0;
  check(
    unsafe { ffi::hv_vcpu_get_reg(cpu.id, register, &mut value) },
    "Read CPU register",
  )?;
  Ok(value)
}

pub fn affinity(cpu: &mut Cpu<'_>, value: u64) -> anyhow::Result<()> {
  check(
    unsafe { ffi::hv_vcpu_set_sys_reg(cpu.id, 0xc005, value) },
    "Set CPU affinity",
  )
}

pub fn enter(cpu: &mut Cpu<'_>, address: u64) -> anyhow::Result<()> {
  set(cpu, ffi::PC, address)?;
  set(cpu, ffi::CPSR, 0x3c5)
}

pub fn run(cpu: &mut Cpu<'_>) -> anyhow::Result<Exit> {
  check(unsafe { ffi::hv_vcpu_run(cpu.id) }, "Run CPU")?;
  let exit = unsafe { *cpu.exit.as_ptr() };
  Ok(match exit.reason {
    0 => Exit::Canceled,
    1 => Exit::Exception {
      syndrome: exit.exception.syndrome,
      virtual_address: exit.exception.virtual_address,
      physical_address: exit.exception.physical_address,
    },
    2 => Exit::Timer,
    _ => Exit::Unknown,
  })
}

impl Drop for Cpu<'_> {
  fn drop(&mut self) {
    unsafe { ffi::hv_vcpu_destroy(self.id) };
  }
}
