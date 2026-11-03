//! owner-thread architectural state for a held CPU; not a complete VM snapshot.

pub mod clock;
mod debug;
mod features;
pub mod group;
pub mod wire;

use super::{check, ffi, get, set, Cpu};
use anyhow::ensure;
use zeroize::{Zeroize, Zeroizing};

// writable EL1/EL0 state, excluding aliases, feature IDs and nested virtualization.
const SYSTEM: &[u16] = &[
  0xc005, 0xc080, 0xc081, 0xc082, 0xc100, 0xc101, 0xc102, 0xc108, 0xc109, 0xc10a, 0xc10b, 0xc110,
  0xc111, 0xc112, 0xc113, 0xc118, 0xc119, 0xc200, 0xc201, 0xc208, 0xc288, 0xc289, 0xc290, 0xc300,
  0xc3a0, 0xc510, 0xc518, 0xc600, 0xc681, 0xc684, 0xc708, 0xd000, 0xde82, 0xde83, 0xdf1a, 0xdf12,
  0xe208, 0xdf19, 0xdf11,
];

// the SDK keeps these CPU interface registers outside the opaque GIC state.
const INTERRUPTS: &[u16] = &[
  0xc230, 0xc643, 0xc644, 0xc648, 0xc663, 0xc664, 0xc665, 0xc666, 0xc667,
];

#[derive(PartialEq, Eq)]
pub struct State {
  features: [u64; 12],
  debug: debug::Debug,
  registers: [u64; 35],
  vectors: [[u8; 16]; 32],
  system: Vec<u64>,
  interrupts: Vec<u64>,
  timer_mask: bool,
  timer_offset: u64,
  serror: bool,
}

impl Drop for State {
  fn drop(&mut self) {
    self.registers.zeroize();
    self.vectors.zeroize();
    self.system.zeroize();
    self.interrupts.zeroize();
    self.timer_mask.zeroize();
    self.timer_offset.zeroize();
    self.serror.zeroize();
  }
}

pub fn vector(cpu: &Cpu<'_>, index: u32) -> anyhow::Result<[u8; 16]> {
  ensure!(index < 32, "Invalid CPU vector register");
  let mut bytes = [0; 16];
  check(
    unsafe { ffi::hopper_simd_get(cpu.id, index, bytes.as_mut_ptr()) },
    "Read CPU vector register",
  )?;
  Ok(bytes)
}

pub fn set_vector(cpu: &mut Cpu<'_>, index: u32, bytes: &[u8; 16]) -> anyhow::Result<()> {
  ensure!(index < 32, "Invalid CPU vector register");
  check(
    unsafe { ffi::hopper_simd_set(cpu.id, index, bytes.as_ptr()) },
    "Set CPU vector register",
  )
}

/// Called on this CPU's owner while it is outside guest execution.
/// Other CPUs must be held when composing this with RAM/device capture.
/// PMU state is not yet covered; do not use this alone to resume desktops.
/// SVE/SME-enabled CPU configurations reject until their wider state is supported.
pub fn capture(cpu: &Cpu<'_>) -> anyhow::Result<State> {
  let features = features::capture(cpu)?;
  let debug = debug::capture(cpu, features[5])?;
  let mut registers = Zeroizing::new([0; 35]);
  for (index, value) in registers.iter_mut().enumerate() {
    *value = get(cpu, index as u32)?;
  }
  let mut vectors = Zeroizing::new([[0; 16]; 32]);
  for (index, value) in vectors.iter_mut().enumerate() {
    *value = vector(cpu, index as u32)?;
  }
  let mut system = Zeroizing::new(Vec::with_capacity(SYSTEM.len()));
  for &register in SYSTEM {
    system.push(read(cpu, register)?);
  }
  let mut interrupts = Zeroizing::new(Vec::with_capacity(INTERRUPTS.len()));
  for &register in INTERRUPTS {
    let mut value = 0;
    check(
      unsafe { ffi::hv_gic_get_icc_reg(cpu.id, register, &mut value) },
      &format!("Read CPU interrupt register 0x{register:04x}"),
    )?;
    interrupts.push(value);
  }
  let mut timer_mask = false;
  let mut timer_offset = 0;
  let mut serror = false;
  check(
    unsafe { ffi::hv_vcpu_get_vtimer_mask(cpu.id, &mut timer_mask) },
    "Read CPU timer mask",
  )?;
  check(
    unsafe { ffi::hv_vcpu_get_vtimer_offset(cpu.id, &mut timer_offset) },
    "Read CPU timer offset",
  )?;
  check(
    unsafe { ffi::hv_vcpu_get_serror(cpu.id, &mut serror) },
    "Read CPU pending SError",
  )?;
  Ok(State {
    features,
    debug,
    registers: *registers,
    vectors: *vectors,
    system: std::mem::take(&mut *system),
    interrupts: std::mem::take(&mut *interrupts),
    timer_mask,
    timer_offset,
    serror,
  })
}

/// Restore on the CPU owner before any CPU resumes. Failure leaves the CPU unusable
/// for resumption; discard the VM rather than executing partially restored state.
/// Timer offset is restored literally; snapshot suspension time needs separate handling.
pub fn restore(cpu: &mut Cpu<'_>, state: &State) -> anyhow::Result<()> {
  restore_inner(cpu, state, None)
}

/// All CPUs must remain held. Use one fresh host tick across every restored CPU.
/// Apple's native GIC applies this offset to both guest counters, so compare values
/// remain in the saved guest epoch. Failure requires VM disposal.
pub fn restore_frozen(
  cpu: &mut Cpu<'_>,
  state: &State,
  clock: &clock::Frozen,
  host: u64,
) -> anyhow::Result<()> {
  restore_inner(cpu, state, Some(clock.rebase(host)))
}

fn restore_inner(cpu: &mut Cpu<'_>, state: &State, clock: Option<u64>) -> anyhow::Result<()> {
  ensure!(
    features::capture(cpu)? == state.features,
    "CPU snapshot features are incompatible"
  );
  ensure!(
    state.system.len() == SYSTEM.len(),
    "Invalid CPU system state"
  );
  ensure!(
    state.interrupts.len() == INTERRUPTS.len(),
    "Invalid CPU interrupt state"
  );
  check(
    unsafe { ffi::hv_vcpu_set_vtimer_offset(cpu.id, clock.unwrap_or(state.timer_offset)) },
    "Restore CPU timer offset",
  )?;
  for (index, &value) in state.registers.iter().enumerate() {
    set(cpu, index as u32, value)?;
  }
  for (index, value) in state.vectors.iter().enumerate() {
    set_vector(cpu, index as u32, value)?;
  }
  debug::restore(cpu, &state.debug)?;
  for (&register, &value) in SYSTEM.iter().zip(&state.system) {
    check(
      unsafe { ffi::hv_vcpu_set_sys_reg(cpu.id, register, value) },
      &format!("Restore CPU system register 0x{register:04x}"),
    )?;
  }
  for (&register, &value) in INTERRUPTS.iter().zip(&state.interrupts) {
    check(
      unsafe { ffi::hv_gic_set_icc_reg(cpu.id, register, value) },
      &format!("Restore CPU interrupt register 0x{register:04x}"),
    )?;
  }
  check(
    unsafe { ffi::hv_vcpu_set_vtimer_mask(cpu.id, state.timer_mask) },
    "Restore CPU timer mask",
  )?;
  check(
    unsafe { ffi::hv_vcpu_set_serror(cpu.id, state.serror) },
    "Restore CPU pending SError",
  )
}

pub(crate) fn read(cpu: &Cpu<'_>, register: u16) -> anyhow::Result<u64> {
  let mut value = 0;
  check(
    unsafe { ffi::hv_vcpu_get_sys_reg(cpu.id, register, &mut value) },
    &format!("Read CPU system register 0x{register:04x}"),
  )?;
  Ok(value)
}
#[cfg(test)]
#[path = "../../tests/cpu/fixture.rs"]
mod fixture;
