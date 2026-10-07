use super::{access, Function, ECAM_SIZE};
use anyhow::{ensure, Context};
use std::collections::BTreeMap;

#[derive(Default)]
pub struct Bus {
  functions: BTreeMap<u8, Function>,
}

impl Bus {
  pub fn attach(&mut self, device: u8, function: u8, config: Function) -> anyhow::Result<()> {
    ensure!(device < 32 && function < 8, "Invalid PCI function address");
    let key = device * 8 + function;
    ensure!(
      !self.functions.contains_key(&key),
      "PCI function is already occupied"
    );
    if function != 0 {
      self
        .functions
        .get_mut(&(device * 8))
        .context("PCI function zero must be attached first")?
        .config[0x0e] |= 0x80;
    }
    self.functions.insert(key, config);
    Ok(())
  }

  pub fn read(&self, offset: u64, width: usize) -> anyhow::Result<u32> {
    ensure!(offset < ECAM_SIZE, "PCI access exceeds the bus window");
    let register = (offset & 0xfff) as usize;
    access(register, width)?;
    match self.functions.get(&((offset >> 12) as u8)) {
      Some(function) => function.read(register, width),
      None => Ok(u32::MAX >> ((4 - width) * 8)),
    }
  }

  pub fn write(&mut self, offset: u64, width: usize, value: u32) -> anyhow::Result<()> {
    ensure!(offset < ECAM_SIZE, "PCI access exceeds the bus window");
    let register = (offset & 0xfff) as usize;
    access(register, width)?;
    if let Some(function) = self.functions.get_mut(&((offset >> 12) as u8)) {
      function.write(register, width, value)?;
    }
    Ok(())
  }
}
