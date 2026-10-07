mod backend;
mod config;
mod registers;

use super::{block, gpu, input, queue};
use crate::{devices::pci::Function, dma::Memory};
use anyhow::ensure;
use backend::Backend;

pub const NOTIFY: u64 = 0x1000;
pub const ISR: u64 = 0x2000;
pub const SPECIFIC: u64 = 0x3000;
pub const VERSION: u64 = 1 << 32;
pub const FLUSH: u64 = 1 << 9;
pub const BLOCK_SIZE: u64 = 1 << 6;
pub const READONLY: u64 = 1 << 5;

pub struct Device {
  pub pci: Function,
  backend: Backend,
  common: [u8; 56],
  driver_features: u64,
  unsupported: bool,
  queues: Vec<QueueState>,
  isr: u8,
  window: [u8; 20],
  window_offset: usize,
  completed: u64,
  fault: Option<String>,
}

struct QueueState {
  registers: [u8; 32],
  queue: Option<queue::Queue>,
}

fn queue_state(index: u16) -> QueueState {
  let mut registers = [0; 32];
  registers[..2].copy_from_slice(&256u16.to_le_bytes());
  registers[2..4].copy_from_slice(&u16::MAX.to_le_bytes());
  registers[6..8].copy_from_slice(&index.to_le_bytes());
  QueueState {
    registers,
    queue: None,
  }
}

fn common(count: u16) -> [u8; 56] {
  let mut bytes = [0; 56];
  bytes[16..18].copy_from_slice(&u16::MAX.to_le_bytes());
  bytes[18..20].copy_from_slice(&count.to_le_bytes());
  bytes
}

pub fn create(disk: block::Disk) -> anyhow::Result<Device> {
  build(Backend::Disk(disk))
}

pub fn graphics(display: gpu::Display) -> anyhow::Result<Device> {
  build(Backend::Gpu(display))
}

pub fn controller(input: input::Input) -> anyhow::Result<Device> {
  build(Backend::Input(input))
}

pub fn send_input(
  device: &mut Device,
  memory: &mut impl Memory,
  events: &[input::Event],
) -> anyhow::Result<()> {
  let Backend::Input(input) = &mut device.backend else {
    anyhow::bail!("Device is not an input controller");
  };
  input::enqueue(input, events)?;
  write(device, memory, NOTIFY, 2, 0)
}

pub fn release_input(device: &mut Device, memory: &mut impl Memory) -> anyhow::Result<()> {
  let Backend::Input(input) = &mut device.backend else {
    anyhow::bail!("Device is not an input controller");
  };
  input::release(input);
  write(device, memory, NOTIFY, 2, 0)
}

pub fn display(device: &Device) -> Option<&gpu::Display> {
  match &device.backend {
    Backend::Gpu(display) => Some(display),
    _ => None,
  }
}

/// Sample the physical framebuffer with every guest CPU stopped.
pub fn refresh(device: &mut Device, memory: &impl Memory) -> anyhow::Result<()> {
  if let Backend::Gpu(display) = &mut device.backend {
    gpu::refresh(display, memory)?;
  }
  Ok(())
}

fn build(backend: Backend) -> anyhow::Result<Device> {
  let (id, class, count, specific) = match &backend {
    Backend::Disk(_) => (0x1042, 0x010000, 1, 64),
    Backend::Gpu(_) => (0x1050, 0x038000, 2, 16),
    Backend::Input(_) => (0x1052, 0x098000, 2, 136),
  };
  let mut pci = Function::new(0x1af4, id, class, 1)?;
  pci.add_bar(0, 0x4000, true)?;
  pci.interrupt_pin(1)?;
  let mut window_offset = 0;
  for (kind, offset, length) in [
    (1, 0, 56u32),
    (2, NOTIFY as u32, count as u32 * 4),
    (3, ISR as u32, 1),
    (4, SPECIFIC as u32, specific),
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
    backend,
    common: common(count),
    driver_features: 0,
    unsupported: false,
    queues: (0..count).map(queue_state).collect(),
    isr: 0,
    window: [0; 20],
    window_offset,
    completed: 0,
    fault: None,
  })
}

pub fn features(device: &Device) -> u64 {
  backend::features(&device.backend)
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
    device.pci.interrupt_status(false);
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
    device.pci.interrupt_status(true);
    device.fault = Some(error.to_string());
  }
  Ok(())
}

fn notify(device: &mut Device, memory: &mut impl Memory, index: usize) -> anyhow::Result<()> {
  if device.common[20] & 0xcf != 15 || !device.pci.bus_master() {
    return Ok(());
  }
  let Some(queue) = &mut device.queues[index].queue else {
    return Ok(());
  };
  while backend::ready(&device.backend, index) {
    let Some(chain) = queue::pop(queue, memory)? else {
      break;
    };
    let length = backend::execute(&mut device.backend, memory, &chain, index)?;
    if queue::complete(queue, memory, &chain, length)? {
      device.isr |= 1;
    }
    device.completed += 1;
  }
  device.pci.interrupt_status(device.isr != 0);
  Ok(())
}
