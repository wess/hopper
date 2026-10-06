//! PCI configuration space for memory-backed native devices.

use anyhow::{ensure, Context};
use std::collections::BTreeMap;

pub const ECAM_SIZE: u64 = 1 << 20;
pub const FIRST_IRQ: u32 = 33;

pub fn interrupt(device: u8, pin: u8) -> anyhow::Result<u32> {
  ensure!(
    device < 32 && (1..=4).contains(&pin),
    "Invalid PCI interrupt pin"
  );
  Ok(FIRST_IRQ + (device as u32 + pin as u32 - 1) % 4)
}

struct Bar {
  slot: usize,
  size: u64,
  wide: bool,
  address: u64,
  probe: [bool; 2],
}

pub struct Function {
  config: [u8; 4096],
  bars: Vec<Bar>,
  capability: usize,
}

impl Function {
  pub fn new(vendor: u16, device: u16, class: u32, revision: u8) -> anyhow::Result<Self> {
    ensure!(
      vendor != 0xffff && class <= 0xffffff,
      "Invalid PCI identity"
    );
    let mut config = [0; 4096];
    config[..2].copy_from_slice(&vendor.to_le_bytes());
    config[2..4].copy_from_slice(&device.to_le_bytes());
    config[8] = revision;
    config[9..12].copy_from_slice(&class.to_le_bytes()[..3]);
    config[0x2c..0x2e].copy_from_slice(&vendor.to_le_bytes());
    config[0x2e..0x30].copy_from_slice(&device.to_le_bytes());
    Ok(Self {
      config,
      bars: Vec::new(),
      capability: 0,
    })
  }

  pub fn add_bar(&mut self, slot: usize, size: u64, wide: bool) -> anyhow::Result<()> {
    ensure!(
      slot < 6
        && (!wide || slot < 5)
        && size >= 16
        && size.is_power_of_two()
        && (wide || size <= 1 << 32),
      "Invalid PCI memory BAR"
    );
    let end = slot + usize::from(wide);
    ensure!(
      self
        .bars
        .iter()
        .all(|bar| { end < bar.slot || slot > bar.slot + usize::from(bar.wide) }),
      "PCI BAR slots overlap"
    );
    self.bars.push(Bar {
      slot,
      size,
      wide,
      address: 0,
      probe: [false; 2],
    });
    Ok(())
  }

  pub fn add_vendor_capability(&mut self, bytes: &[u8]) -> anyhow::Result<usize> {
    ensure!(bytes.len() >= 2, "PCI capability is too short");
    let start = if self.capability == 0 {
      0x40
    } else {
      let length = self.config[self.capability + 2] as usize;
      ensure!(length >= 3, "Previous capability has no length");
      (self.capability + length).next_multiple_of(4)
    };
    ensure!(
      bytes.len() >= 3 && bytes[2] as usize == bytes.len(),
      "PCI vendor capability length is invalid"
    );
    ensure!(
      bytes[0] == 9 && start + bytes.len() <= 0x100,
      "PCI vendor capability does not fit"
    );
    if self.capability == 0 {
      self.config[0x34] = start as u8;
      self.config[6] |= 0x10;
    } else {
      self.config[self.capability + 1] = start as u8;
    }
    self.config[start..start + bytes.len()].copy_from_slice(bytes);
    self.config[start + 1] = 0;
    self.capability = start;
    Ok(start)
  }

  fn byte(&self, offset: usize) -> u8 {
    if (0x10..0x28).contains(&offset) {
      let slot = (offset - 0x10) / 4;
      if let Some(bar) = self
        .bars
        .iter()
        .find(|bar| slot == bar.slot || (bar.wide && slot == bar.slot + 1))
      {
        let half = slot - bar.slot;
        let value = if bar.probe[half] {
          !(bar.size - 1)
        } else {
          bar.address
        };
        let value = if half == 0 {
          (value as u32 & 0xfffffff0) | if bar.wide { 4 } else { 0 }
        } else {
          (value >> 32) as u32
        };
        return value.to_le_bytes()[offset % 4];
      }
      return 0;
    }
    self.config[offset]
  }

  pub fn read(&self, offset: usize, width: usize) -> anyhow::Result<u32> {
    access(offset, width)?;
    Ok((0..width).fold(0, |value, index| {
      value | (self.byte(offset + index) as u32) << (index * 8)
    }))
  }

  pub fn write(&mut self, offset: usize, width: usize, value: u32) -> anyhow::Result<()> {
    access(offset, width)?;
    if (0x10..0x28).contains(&offset) {
      let slot = (offset - 0x10) / 4;
      if let Some(bar) = self
        .bars
        .iter_mut()
        .find(|bar| slot == bar.slot || (bar.wide && slot == bar.slot + 1))
      {
        let half = slot - bar.slot;
        let shift = half * 32;
        let mask = (u32::MAX >> ((4 - width) * 8)) << ((offset % 4) * 8);
        let old = (bar.address >> shift) as u32 | if half == 0 && bar.wide { 4 } else { 0 };
        let next = (old & !mask) | ((value << ((offset % 4) * 8)) & mask);
        bar.probe[half] = next == u32::MAX;
        if !bar.probe[half] {
          let mask = (u32::MAX as u64) << shift;
          bar.address = ((bar.address & !mask) | ((next as u64) << shift)) & !(bar.size - 1);
        }
      }
      return Ok(());
    }
    for (index, byte) in value.to_le_bytes().iter().take(width).enumerate() {
      let address = offset + index;
      match address {
        4 => self.config[4] = byte & 6,
        5 => self.config[5] = byte & 4,
        7 => self.config[7] &= !(byte & 0xf9),
        0x3c => self.config[0x3c] = *byte,
        _ => {}
      }
    }
    Ok(())
  }

  pub fn memory(&self, address: u64) -> Option<(usize, u64)> {
    if self.config[4] & 2 == 0 {
      return None;
    }
    self.bars.iter().find_map(|bar| {
      let offset = address.checked_sub(bar.address)?;
      (bar.address != 0 && offset < bar.size).then_some((bar.slot, offset))
    })
  }

  pub fn bus_master(&self) -> bool {
    self.config[4] & 4 != 0
  }
}

fn access(offset: usize, width: usize) -> anyhow::Result<()> {
  ensure!(
    matches!(width, 1 | 2 | 4)
      && offset.is_multiple_of(width)
      && offset.checked_add(width).is_some_and(|end| end <= 4096),
    "Invalid PCI configuration access"
  );
  Ok(())
}

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
