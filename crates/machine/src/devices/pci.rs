//! PCI configuration space for memory-backed native devices.

use anyhow::{ensure, Context};
mod bus;
pub use bus::Bus;

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
  next_capability: usize,
  msix: Option<usize>,
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
      next_capability: 0x40,
      msix: None,
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
    ensure!(
      bytes.len() >= 3 && bytes[2] as usize == bytes.len(),
      "PCI vendor capability length is invalid"
    );
    ensure!(bytes[0] == 9, "PCI vendor capability does not fit");
    self.add_capability(bytes)
  }

  pub fn add_msix(
    &mut self,
    count: u16,
    slot: usize,
    table: u32,
    pending: u32,
  ) -> anyhow::Result<usize> {
    ensure!(
      self.msix.is_none()
        && (1..=2048).contains(&count)
        && table.is_multiple_of(8)
        && pending.is_multiple_of(8),
      "Invalid MSI-X capability"
    );
    let bar = self
      .bars
      .iter()
      .find(|bar| bar.slot == slot)
      .context("MSI-X BAR is missing")?;
    let table_end = table as u64 + count as u64 * 16;
    let pending_end = pending as u64 + (count as u64).div_ceil(64) * 8;
    ensure!(
      table_end <= bar.size
        && pending_end <= bar.size
        && (table_end <= pending as u64 || pending_end <= table as u64),
      "MSI-X tables overlap or exceed BAR"
    );
    let mut bytes = vec![0x11, 0];
    bytes.extend((count - 1).to_le_bytes());
    bytes.extend((table | slot as u32).to_le_bytes());
    bytes.extend((pending | slot as u32).to_le_bytes());
    let offset = self.add_capability(&bytes)?;
    self.msix = Some(offset);
    Ok(offset)
  }

  fn add_capability(&mut self, bytes: &[u8]) -> anyhow::Result<usize> {
    let start = self.next_capability;
    ensure!(start + bytes.len() <= 0x100, "PCI capability does not fit");
    if self.capability == 0 {
      self.config[0x34] = start as u8;
      self.config[6] |= 0x10;
    } else {
      self.config[self.capability + 1] = start as u8;
    }
    self.config[start..start + bytes.len()].copy_from_slice(bytes);
    self.config[start + 1] = 0;
    self.capability = start;
    self.next_capability = (start + bytes.len()).next_multiple_of(4);
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
        _ if self.msix.is_some_and(|offset| address == offset + 3) => {
          self.config[address] = (self.config[address] & 0x3f) | (byte & 0xc0);
        }
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

  pub fn interrupt_pin(&mut self, pin: u8) -> anyhow::Result<()> {
    ensure!(pin <= 4, "Invalid PCI interrupt pin");
    self.config[0x3d] = pin;
    Ok(())
  }

  pub fn interrupt_status(&mut self, pending: bool) {
    self.config[6] = (self.config[6] & !8) | if pending { 8 } else { 0 };
  }

  pub fn interrupt_enabled(&self) -> bool {
    self.config[5] & 4 == 0
  }

  pub fn msix_enabled(&self) -> bool {
    self
      .msix
      .is_some_and(|offset| self.config[offset + 3] & 0x80 != 0)
  }

  pub fn msix_masked(&self) -> bool {
    self
      .msix
      .is_some_and(|offset| self.config[offset + 3] & 0x40 != 0)
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
