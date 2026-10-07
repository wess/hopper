use super::{
  backend, gpu, interrupt_status, msix, notify, raise, registers, Backend, Device, ISR, NOTIFY,
  SPECIFIC,
};
use crate::dma::Memory;
use anyhow::ensure;

pub fn memory_read(
  device: &mut Device,
  bar: usize,
  offset: u64,
  width: usize,
) -> anyhow::Result<u64> {
  match bar {
    0 => read(device, offset, width).map(u64::from),
    2 => msix::read(&device.msix, offset, width),
    _ => anyhow::bail!("Invalid Virtio memory BAR"),
  }
}

pub fn memory_write(
  device: &mut Device,
  memory: &mut impl Memory,
  bar: usize,
  offset: u64,
  width: usize,
  value: u64,
) -> anyhow::Result<()> {
  match bar {
    0 => write(device, memory, offset, width, value as u32),
    2 => msix::write(&mut device.msix, offset, width, value),
    _ => anyhow::bail!("Invalid Virtio memory BAR"),
  }
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
  if (gpu::linear::BASE..gpu::linear::BASE + 32).contains(&offset) && width == 4 {
    if let Backend::Gpu(display) = &device.backend {
      return Ok(gpu::linear_read(display, offset - gpu::linear::BASE));
    }
  }
  if offset < 56 && offset + width as u64 <= 56 {
    return registers::read(device, offset as usize, width);
  }
  if offset == ISR && width == 1 {
    let value = device.isr;
    device.isr = 0;
    interrupt_status(device);
    return Ok(value as u32);
  }
  let length = backend::config_length(&device.backend);
  if (SPECIFIC..SPECIFIC + length).contains(&offset) && offset + width as u64 <= SPECIFIC + length {
    let bytes = backend::config(&device.backend);
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
  let result = if (gpu::linear::BASE..gpu::linear::BASE + 32).contains(&offset) && width == 4 {
    if let Backend::Gpu(display) = &mut device.backend {
      gpu::linear_write(display, memory, offset - gpu::linear::BASE, value)
    } else {
      Ok(())
    }
  } else if offset < 56 && offset + width as u64 <= 56 {
    registers::write(device, memory, offset as usize, width, value)
  } else if (NOTIFY..NOTIFY + device.queues.len() as u64 * 4).contains(&offset)
    && offset.is_multiple_of(4)
    && width == 2
    && value as usize == ((offset - NOTIFY) / 4) as usize
  {
    notify(device, memory, value as usize)
  } else if (SPECIFIC..SPECIFIC + backend::config_length(&device.backend)).contains(&offset) {
    backend::configure(
      &mut device.backend,
      (offset - SPECIFIC) as usize,
      width,
      value,
    );
    Ok(())
  } else {
    Ok(())
  };
  if let Err(error) = result {
    device.common[20] |= 64;
    device.isr |= 2;
    raise(
      device,
      u16::from_le_bytes(device.common[16..18].try_into()?),
    );
    interrupt_status(device);
    device.fault = Some(error.to_string());
  }
  Ok(())
}
