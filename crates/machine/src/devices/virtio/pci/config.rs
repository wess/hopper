use super::{
  backend, read as register_read, write as register_write, Device, ISR, NOTIFY, SPECIFIC,
};
use crate::dma::Memory;

fn target(device: &Device) -> Option<(u64, usize)> {
  let width = u32::from_le_bytes(device.window[12..16].try_into().ok()?) as usize;
  let offset = u32::from_le_bytes(device.window[8..12].try_into().ok()?) as u64;
  if device.window[4] != 0 || !matches!(width, 1 | 2 | 4) || !offset.is_multiple_of(width as u64) {
    return None;
  }
  [
    (0, 56),
    (NOTIFY, device.queues.len() as u64 * 4),
    (ISR, 1),
    (SPECIFIC, backend::config_length(&device.backend)),
  ]
  .into_iter()
  .any(|(base, length)| offset >= base && offset + width as u64 <= base + length)
  .then_some((offset, width))
}

pub(super) fn read(device: &mut Device, offset: usize, width: usize) -> anyhow::Result<u32> {
  device.pci.read(offset, width)?;
  let start = device.window_offset;
  if offset < start + 20 && offset + width > start + 16 {
    let value = match target(device) {
      Some((offset, width)) => register_read(device, offset, width)?,
      None => 0,
    };
    device.window[16..20].copy_from_slice(&value.to_le_bytes());
  }
  let mut value = 0;
  for index in 0..width {
    let address = offset + index;
    let byte = if (start..start + 20).contains(&address) {
      let field = address - start;
      if field == 4 || field >= 8 {
        device.window[field]
      } else {
        device.pci.read(address, 1)? as u8
      }
    } else {
      device.pci.read(address, 1)? as u8
    };
    value |= (byte as u32) << (index * 8);
  }
  Ok(value)
}

pub(super) fn write(
  device: &mut Device,
  memory: &mut impl Memory,
  offset: usize,
  width: usize,
  value: u32,
) -> anyhow::Result<()> {
  device.pci.write(offset, width, value)?;
  let start = device.window_offset;
  for (index, byte) in value.to_le_bytes().iter().take(width).enumerate() {
    let address = offset + index;
    if (start..start + 20).contains(&address) {
      let field = address - start;
      if field == 4 || field >= 8 {
        device.window[field] = *byte;
      }
    }
  }
  if offset < start + 20 && offset + width > start + 16 {
    if let Some((offset, width)) = target(device) {
      let value = u32::from_le_bytes(device.window[16..20].try_into()?);
      register_write(device, memory, offset, width, value)?;
    }
  }
  Ok(())
}
