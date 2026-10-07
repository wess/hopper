use super::{commands::word, resource, Display};
use crate::{
  devices::virtio::queue::Chain,
  dma::{self, Memory},
};
use anyhow::{ensure, Context};

#[derive(Default)]
pub struct Cursor {
  pub x: u32,
  pub y: u32,
  pub hot_x: u32,
  pub hot_y: u32,
  pub rgba: Option<Vec<u8>>,
  pub generation: u64,
}

pub fn execute(
  display: &mut Display,
  memory: &mut impl Memory,
  chain: &Chain,
) -> anyhow::Result<u32> {
  let readable: Vec<_> = chain
    .buffers
    .iter()
    .filter(|b| !b.writable)
    .map(|b| b.span)
    .collect();
  let writable: Vec<_> = chain
    .buffers
    .iter()
    .filter(|b| b.writable)
    .map(|b| b.span)
    .collect();
  let mut data = [0; 56];
  dma::read(memory, &readable, 0, &mut data)?;
  let command = word(&data, 0);
  ensure!(
    matches!(command, 0x300 | 0x301),
    "Invalid GPU cursor command"
  );
  ensure!(
    word(&data, 4) & !1 == 0 && word(&data, 16) == 0 && word(&data, 24) == 0,
    "Unsupported GPU cursor context or scanout"
  );
  let response = dma::length(&writable);
  ensure!(
    response == 0 || response >= 24,
    "Truncated GPU cursor response"
  );
  if command == 0x300 {
    let id = word(&data, 40);
    if id == 0 {
      display.cursor.rgba = None;
    } else {
      let resource = display
        .resources
        .get(&id)
        .context("Unknown GPU cursor resource")?;
      let hot_x = word(&data, 44);
      let hot_y = word(&data, 48);
      ensure!(
        resource.width == 64 && resource.height == 64 && hot_x < 64 && hot_y < 64,
        "Invalid GPU cursor dimensions or hotspot"
      );
      let mut rgba = Vec::with_capacity(64 * 64 * 4);
      for pixel in resource.pixels.as_chunks::<4>().0 {
        rgba.extend(resource::rgba(resource.format, pixel));
      }
      display.cursor.rgba = Some(rgba);
      display.cursor.hot_x = hot_x;
      display.cursor.hot_y = hot_y;
    }
  }
  display.cursor.x = word(&data, 28);
  display.cursor.y = word(&data, 32);
  display.cursor.generation = display.cursor.generation.wrapping_add(1);
  if response == 0 {
    return Ok(0);
  }
  let mut reply = [0; 24];
  reply[..4].copy_from_slice(&0x1100u32.to_le_bytes());
  if word(&data, 4) & 1 != 0 {
    reply[4..16].copy_from_slice(&data[4..16]);
  }
  dma::write(memory, &writable, 0, &reply)?;
  Ok(24)
}
