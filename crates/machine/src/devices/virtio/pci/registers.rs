use super::{block, common, features, queue, Device, FLUSH, VERSION};
use crate::dma::Memory;
use anyhow::ensure;

fn word(bytes: &[u8], offset: usize) -> u16 {
  u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap())
}

fn dword(bytes: &[u8], offset: usize) -> u32 {
  u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

pub(super) fn read(device: &Device, offset: usize, width: usize) -> anyhow::Result<u32> {
  let mut bytes = device.common;
  let selected = dword(&bytes, 0);
  let offered = if selected < 2 {
    (features(device) >> (selected * 32)) as u32
  } else {
    0
  };
  bytes[4..8].copy_from_slice(&offered.to_le_bytes());
  let selected = dword(&bytes, 8);
  let accepted = if selected < 2 {
    (device.driver_features >> (selected * 32)) as u32
  } else {
    0
  };
  bytes[12..16].copy_from_slice(&accepted.to_le_bytes());
  if word(&bytes, 22) != 0 {
    bytes[24..].fill(0);
    bytes[26..28].copy_from_slice(&u16::MAX.to_le_bytes());
  }
  Ok(
    bytes[offset..offset + width]
      .iter()
      .enumerate()
      .fold(0, |value, (index, byte)| {
        value | (*byte as u32) << (index * 8)
      }),
  )
}

pub(super) fn write(
  device: &mut Device,
  memory: &mut impl Memory,
  offset: usize,
  width: usize,
  value: u32,
) -> anyhow::Result<()> {
  match (offset, width) {
    (0 | 8, 4) | (22, 2) => {
      device.common[offset..offset + width].copy_from_slice(&value.to_le_bytes()[..width])
    }
    (12, 4) if device.common[20] & 8 == 0 => {
      let select = dword(&device.common, 8);
      if select < 2 {
        let shift = select * 32;
        device.driver_features =
          (device.driver_features & !(0xffffffffu64 << shift)) | ((value as u64) << shift);
      } else if value != 0 {
        device.unsupported = true;
      }
    }
    (20, 1) => {
      if value as u8 == 0 {
        block::writeback(&mut device.disk, false)?;
        device.common = common();
        device.driver_features = 0;
        device.unsupported = false;
        device.queue = None;
        device.isr = 0;
        device.pci.interrupt_status(false);
        device.fault = None;
      } else {
        let mut next = value as u8 & 0x8f | device.common[20] & 64;
        if next & 8 != 0
          && (device.driver_features & VERSION == 0
            || device.driver_features & !features(device) != 0
            || device.unsupported
            || next & 3 != 3)
        {
          next &= !12;
        }
        if next & 4 != 0 && next & 8 == 0 {
          next &= !4;
        }
        block::writeback(
          &mut device.disk,
          next & 8 != 0 && device.driver_features & FLUSH != 0,
        )?;
        device.common[20] = next;
      }
    }
    (24, 2) if word(&device.common, 22) == 0 && device.queue.is_none() => {
      ensure!(
        value > 0 && value <= 256 && value.is_power_of_two(),
        "Invalid Virtio block queue size"
      );
      device.common[24..26].copy_from_slice(&(value as u16).to_le_bytes());
    }
    (28, 2) if word(&device.common, 22) == 0 && device.queue.is_none() && value == 1 => {
      let mut addresses = [0; 3];
      for (index, address) in addresses.iter_mut().enumerate() {
        *address = u64::from_le_bytes(device.common[32 + index * 8..40 + index * 8].try_into()?);
      }
      device.queue = Some(queue::create(
        memory,
        word(&device.common, 24),
        addresses[0],
        addresses[1],
        addresses[2],
      )?);
      device.common[28..30].copy_from_slice(&1u16.to_le_bytes());
    }
    (32 | 36 | 40 | 44 | 48 | 52, 4) if word(&device.common, 22) == 0 && device.queue.is_none() => {
      device.common[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    _ => {}
  }
  Ok(())
}
