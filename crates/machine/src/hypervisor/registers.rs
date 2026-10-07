use super::{get, set, Cpu};
use anyhow::ensure;

#[derive(Clone)]
pub struct Registers {
  values: [u64; 32],
}

pub fn capture(cpu: &Cpu<'_>) -> anyhow::Result<Registers> {
  let mut values = [0; 32];
  for (index, value) in values.iter_mut().enumerate() {
    *value = get(cpu, index as u32)?;
  }
  Ok(Registers { values })
}

pub fn read(registers: &Registers, index: u32) -> anyhow::Result<u64> {
  ensure!(index < 32, "Invalid emulated CPU register");
  Ok(registers.values[index as usize])
}

pub fn write(registers: &mut Registers, index: u32, value: u64) -> anyhow::Result<()> {
  ensure!(index < 32, "Invalid emulated CPU register");
  registers.values[index as usize] = value;
  Ok(())
}

pub fn apply(cpu: &mut Cpu<'_>, before: &Registers, after: &Registers) -> anyhow::Result<()> {
  for (index, (old, new)) in before.values.iter().zip(after.values).enumerate() {
    if *old != new {
      set(cpu, index as u32, new)?;
    }
  }
  Ok(())
}
