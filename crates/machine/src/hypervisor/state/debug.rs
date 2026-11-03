use super::read;
use crate::hypervisor::{check, ffi, Cpu};
use anyhow::ensure;
use zeroize::Zeroize;

#[derive(PartialEq, Eq)]
pub(super) struct Debug(Vec<(u16, u64)>);

impl Drop for Debug {
  fn drop(&mut self) {
    for (_, value) in &mut self.0 {
      value.zeroize();
    }
  }
}

fn registers(features: u64) -> Vec<u16> {
  let mut registers = vec![0x8010, 0x8012];
  for (count, base) in [
    (((features >> 12) & 15) + 1, 0x8004),
    (((features >> 20) & 15) + 1, 0x8006),
  ] {
    for index in 0..count as u16 {
      registers.extend([base + index * 8, base + index * 8 + 1]);
    }
  }
  registers
}

impl Debug {
  pub(super) fn values(&self) -> &[(u16, u64)] {
    &self.0
  }

  pub(super) fn validate(&self, features: u64) -> anyhow::Result<()> {
    let expected = registers(features);
    ensure!(
      self.0.len() == expected.len()
        && self
          .0
          .iter()
          .zip(expected)
          .all(|(&(id, _), expected)| id == expected),
      "CPU debug schema is incompatible"
    );
    Ok(())
  }

  pub(super) fn from_values(features: u64, values: Vec<(u16, u64)>) -> anyhow::Result<Self> {
    let state = Self(values);
    state.validate(features)?;
    Ok(state)
  }
}

pub(super) fn capture(cpu: &Cpu<'_>, features: u64) -> anyhow::Result<Debug> {
  let registers = registers(features);
  let mut state = Debug(Vec::with_capacity(registers.len()));
  for register in registers {
    state.0.push((register, read(cpu, register)?));
  }
  Ok(state)
}

pub(super) fn restore(cpu: &mut Cpu<'_>, state: &Debug) -> anyhow::Result<()> {
  for &(register, value) in &state.0 {
    check(
      unsafe { ffi::hv_vcpu_set_sys_reg(cpu.id, register, value) },
      &format!("Restore CPU debug register 0x{register:04x}"),
    )?;
  }
  Ok(())
}
