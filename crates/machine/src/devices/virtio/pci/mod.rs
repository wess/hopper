mod config;
mod registers;

use super::{block, queue};
use crate::{devices::pci::Function, dma::Memory};
use anyhow::ensure;

pub const NOTIFY: u64 = 0x1000;
pub const ISR: u64 = 0x2000;
pub const SPECIFIC: u64 = 0x3000;
pub const VERSION: u64 = 1 << 32;
pub const FLUSH: u64 = 1 << 9;
pub const BLOCK_SIZE: u64 = 1 << 6;
pub const READONLY: u64 = 1 << 5;

pub struct Device {
  pub pci: Function,
  disk: block::Disk,
  common: [u8; 56],
  driver_features: u64,
  unsupported: bool,
  queue: Option<queue::Queue>,
  isr: u8,
  window: [u8; 20],
  window_offset: usize,
  completed: u64,
  fault: Option<String>,
}

fn common() -> [u8; 56] {
  let mut bytes = [0; 56];
  bytes[16..18].copy_from_slice(&u16::MAX.to_le_bytes());
  bytes[18..20].copy_from_slice(&1u16.to_le_bytes());
  bytes[24..26].copy_from_slice(&256u16.to_le_bytes());
  bytes[26..28].copy_from_slice(&u16::MAX.to_le_bytes());
  bytes
}

pub fn create(disk: block::Disk) -> anyhow::Result<Device> {
  let mut pci = Function::new(0x1af4, 0x1042, 0x010000, 1)?;
  pci.add_bar(0, 0x4000, true)?;
  pci.interrupt_pin(1)?;
  let mut window_offset = 0;
  for (kind, offset, length) in [
    (1, 0, 56u32),
    (2, NOTIFY as u32, 2),
    (3, ISR as u32, 1),
    (4, SPECIFIC as u32, 64),
    (5, 0, 0),
  ] {
    let size = if kind == 2 || kind == 5 { 20 } else { 16 };
    let mut capability = vec![9, 0, size, kind, 0, 0, 0, 0];
    capability.extend(offset.to_le_bytes());
    capability.extend(length.to_le_bytes());
    if kind == 2 {
      capability.extend(4u32.to_le_bytes());
    }
    if kind == 5 {
      capability.extend([0; 4]);
    }
    let position = pci.add_vendor_capability(&capability)?;
    if kind == 5 {
      window_offset = position;
    }
  }
  Ok(Device {
    pci,
    disk,
    common: common(),
    driver_features: 0,
    unsupported: false,
    queue: None,
    isr: 0,
    window: [0; 20],
    window_offset,
    completed: 0,
    fault: None,
  })
}

pub fn features(device: &Device) -> u64 {
  VERSION
    | FLUSH
    | BLOCK_SIZE
    | if block::readonly(&device.disk) {
      READONLY
    } else {
      0
    }
}

pub fn interrupt(device: &Device) -> bool {
  device.isr != 0 && device.pci.interrupt_enabled()
}

pub fn completed(device: &Device) -> u64 {
  device.completed
}
pub fn fault(device: &Device) -> Option<&str> {
  device.fault.as_deref()
}

pub fn config_read(device: &mut Device, offset: usize, width: usize) -> anyhow::Result<u32> {
  config::read(device, offset, width)
}

pub fn config_write(
  device: &mut Device,
  memory: &mut impl Memory,
  offset: usize,
  width: usize,
  value: u32,
) -> anyhow::Result<()> {
  config::write(device, memory, offset, width, value)
}

pub(super) fn access(offset: u64, width: usize) -> anyhow::Result<()> {
  ensure!(
    matches!(width, 1 | 2 | 4)
      && offset.is_multiple_of(width as u64)
      && offset
        .checked_add(width as u64)
        .is_some_and(|end| end <= 0x4000),
    "Invalid Virtio PCI register access"
  );
  Ok(())
}

pub fn read(device: &mut Device, offset: u64, width: usize) -> anyhow::Result<u32> {
  access(offset, width)?;
  if offset < 56 && offset + width as u64 <= 56 {
    return registers::read(device, offset as usize, width);
  }
  if offset == ISR && width == 1 {
    let value = device.isr;
    device.isr = 0;
    device.pci.interrupt_status(false);
    return Ok(value as u32);
  }
  if (SPECIFIC..SPECIFIC + 64).contains(&offset) && offset + width as u64 <= SPECIFIC + 64 {
    let mut bytes = [0; 64];
    bytes[..8].copy_from_slice(&block::capacity(&device.disk).to_le_bytes());
    bytes[20..24].copy_from_slice(&512u32.to_le_bytes());
    let start = (offset - SPECIFIC) as usize;
    return Ok(
      bytes[start..start + width]
        .iter()
        .enumerate()
        .fold(0, |value, (index, byte)| {
          value | (*byte as u32) << (index * 8)
        }),
    );
  }
  Ok(0)
}

pub fn write(
  device: &mut Device,
  memory: &mut impl Memory,
  offset: u64,
  width: usize,
  value: u32,
) -> anyhow::Result<()> {
  access(offset, width)?;
  let value = value & (u32::MAX >> ((4 - width) * 8));
  let result = if offset < 56 && offset + width as u64 <= 56 {
    registers::write(device, memory, offset as usize, width, value)
  } else if offset == NOTIFY && width == 2 && value as u16 == 0 {
    notify(device, memory)
  } else {
    Ok(())
  };
  if let Err(error) = result {
    device.common[20] |= 64;
    device.isr |= 2;
    device.pci.interrupt_status(true);
    device.fault = Some(error.to_string());
  }
  Ok(())
}

fn notify(device: &mut Device, memory: &mut impl Memory) -> anyhow::Result<()> {
  if device.common[20] & 0xcf != 15 || !device.pci.bus_master() {
    return Ok(());
  }
  let Some(queue) = &mut device.queue else {
    return Ok(());
  };
  while let Some(chain) = queue::pop(queue, memory)? {
    let length = block::execute(&mut device.disk, memory, &chain)?;
    if queue::complete(queue, memory, &chain, length)? {
      device.isr |= 1;
    }
    device.completed += 1;
  }
  device.pci.interrupt_status(device.isr != 0);
  Ok(())
}
