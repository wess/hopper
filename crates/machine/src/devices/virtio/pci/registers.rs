use super::{backend, common, features, queue, queue_state, Device, VERSION};
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
  if let Some(state) = device.queues.get(word(&bytes, 22) as usize) {
    bytes[24..].copy_from_slice(&state.registers);
  } else {
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
        backend::status(&mut device.backend, 0, true)?;
        device.common = common(device.queues.len() as u16);
        device.driver_features = 0;
        device.unsupported = false;
        for (index, state) in device.queues.iter_mut().enumerate() {
          *state = queue_state(index as u16);
        }
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
        backend::status(
          &mut device.backend,
          if next & 8 != 0 {
            device.driver_features
          } else {
            0
          },
          false,
        )?;
        device.common[20] = next;
      }
    }
    (24 | 28, 2) | (32 | 36 | 40 | 44 | 48 | 52, 4) => {
      let selected = word(&device.common, 22) as usize;
      let Some(state) = device.queues.get_mut(selected) else {
        return Ok(());
      };
      if state.queue.is_some() {
        return Ok(());
      }
      match offset {
        24 => {
          ensure!(
            value > 0 && value <= 256 && value.is_power_of_two(),
            "Invalid Virtio queue size"
          );
          state.registers[..2].copy_from_slice(&(value as u16).to_le_bytes());
        }
        28 if value == 1 => {
          let mut addresses = [0; 3];
          for (index, address) in addresses.iter_mut().enumerate() {
            *address =
              u64::from_le_bytes(state.registers[8 + index * 8..16 + index * 8].try_into()?);
          }
          state.queue = Some(queue::create(
            memory,
            word(&state.registers, 0),
            addresses[0],
            addresses[1],
            addresses[2],
          )?);
          state.registers[4..6].copy_from_slice(&1u16.to_le_bytes());
        }
        32..=52 => {
          state.registers[offset - 24..offset - 24 + 4].copy_from_slice(&value.to_le_bytes())
        }
        _ => {}
      }
    }
    _ => {}
  }
  Ok(())
}
