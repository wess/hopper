use super::pci::{self, Device};
use crate::{devices::pci as config, dma::Memory};
use anyhow::{ensure, Context};

fn address(offset: u64, width: usize) -> anyhow::Result<(usize, usize, bool)> {
  ensure!(
    offset < config::ECAM_SIZE,
    "PCI access exceeds the bus window"
  );
  let register = (offset & 0xfff) as usize;
  config::access(register, width)?;
  Ok(((offset >> 15) as usize, register, offset & 0x7000 == 0))
}

pub fn read(devices: &mut [Option<Device>], offset: u64, width: usize) -> anyhow::Result<u32> {
  let (slot, register, zero) = address(offset, width)?;
  match devices
    .get_mut(slot)
    .and_then(Option::as_mut)
    .filter(|_| zero)
  {
    Some(device) => pci::config_read(device, register, width),
    None => Ok(u32::MAX >> ((4 - width) * 8)),
  }
}

pub fn write(
  devices: &mut [Option<Device>],
  memory: &mut impl Memory,
  offset: u64,
  width: usize,
  value: u32,
) -> anyhow::Result<()> {
  let (slot, register, zero) = address(offset, width)?;
  if let Some(device) = devices
    .get_mut(slot)
    .and_then(Option::as_mut)
    .filter(|_| zero)
  {
    pci::config_write(device, memory, register, width, value)?;
  }
  Ok(())
}

/// resolve once before dispatch; overlapping guest-programmed BARs have no owner.
pub fn mapped(devices: &[Option<Device>], address: u64) -> anyhow::Result<Option<usize>> {
  let mut owner = None;
  for (slot, device) in devices.iter().enumerate() {
    if device
      .as_ref()
      .is_some_and(|device| device.pci.memory(address).is_some())
    {
      ensure!(owner.is_none(), "Guest PCI memory BARs overlap");
      owner = Some(slot);
    }
  }
  Ok(owner)
}

pub fn memory_read(
  devices: &mut [Option<Device>],
  address: u64,
  width: usize,
) -> anyhow::Result<(usize, u64)> {
  let slot = mapped(devices, address)?.context("Unmapped guest PCI memory")?;
  let device = devices[slot].as_mut().context("Missing PCI device")?;
  let (bar, offset) = device
    .pci
    .memory(address)
    .context("Unmapped guest PCI BAR")?;
  Ok((slot, pci::memory_read(device, bar, offset, width)?))
}

pub fn memory_write(
  devices: &mut [Option<Device>],
  memory: &mut impl Memory,
  address: u64,
  width: usize,
  value: u64,
) -> anyhow::Result<usize> {
  let slot = mapped(devices, address)?.context("Unmapped guest PCI memory")?;
  let device = devices[slot].as_mut().context("Missing PCI device")?;
  let (bar, offset) = device
    .pci
    .memory(address)
    .context("Unmapped guest PCI BAR")?;
  pci::memory_write(device, memory, bar, offset, width, value)?;
  Ok(slot)
}
