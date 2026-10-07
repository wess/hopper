use super::{resource, Frame};
use crate::dma::Memory;
use anyhow::{ensure, Context};

pub const BASE: u64 = 0x3800;
pub const MAGIC: u32 = 0x48504642;

#[derive(Default)]
pub(super) struct Linear {
  registers: [u32; 6],
  layout: Option<Layout>,
  pub frame: Option<Frame>,
}

#[derive(Clone, Copy)]
struct Layout {
  address: u64,
  width: u32,
  height: u32,
  stride: u32,
}

pub(super) fn active(linear: &Linear) -> bool {
  linear.layout.is_some()
}

pub(super) fn disable(linear: &mut Linear) {
  linear.layout = None;
  linear.frame = None;
  linear.registers[5] = 0;
}

pub(super) fn read(linear: &Linear, offset: u64) -> u32 {
  match offset {
    0 => MAGIC,
    4 => 1,
    8..=28 => linear.registers[((offset - 8) / 4) as usize],
    _ => 0,
  }
}

pub(super) fn write(
  linear: &mut Linear,
  memory: &impl Memory,
  offset: u64,
  value: u32,
) -> anyhow::Result<()> {
  if !(8..=28).contains(&offset) {
    return Ok(());
  }
  if offset != 28 {
    linear.registers[((offset - 8) / 4) as usize] = value;
    return Ok(());
  }
  if value == 0 {
    disable(linear);
    return Ok(());
  }
  ensure!(value == 1, "Invalid linear framebuffer enable value");
  let [low, high, width, height, stride, _] = linear.registers;
  let address = low as u64 | (high as u64) << 32;
  ensure!(
    resource::size(width, height).is_some(),
    "Invalid linear framebuffer dimensions"
  );
  ensure!(
    stride >= width && stride <= 8192,
    "Invalid linear framebuffer stride"
  );
  let length = stride as usize * height as usize * 4;
  ensure!(
    length <= super::LIMIT && address.is_multiple_of(4096) && memory.contains(address, length),
    "Linear framebuffer exceeds guest RAM"
  );
  linear.layout = Some(Layout {
    address,
    width,
    height,
    stride,
  });
  linear.registers[5] = 1;
  linear.frame = None;
  Ok(())
}

// the caller must pause every CPU before sampling guest RAM.
pub(super) fn refresh(linear: &mut Linear, memory: &impl Memory) -> anyhow::Result<()> {
  let Some(layout) = linear.layout else {
    return Ok(());
  };
  let size = layout.width as usize * layout.height as usize * 4;
  let mut rgba = Vec::new();
  rgba
    .try_reserve_exact(size)
    .context("Allocate linear framebuffer capture")?;
  rgba.resize(size, 0);
  for y in 0..layout.height as usize {
    let row = &mut rgba[y * layout.width as usize * 4..(y + 1) * layout.width as usize * 4];
    memory.read(layout.address + y as u64 * layout.stride as u64 * 4, row)?;
    for pixel in row.as_chunks_mut::<4>().0 {
      pixel.swap(0, 2);
      pixel[3] = 255;
    }
  }
  if linear
    .frame
    .as_ref()
    .is_some_and(|frame| frame.rgba == rgba)
  {
    return Ok(());
  }
  let generation = linear
    .frame
    .as_ref()
    .map_or(1, |frame| frame.generation.wrapping_add(1));
  linear.frame = Some(Frame {
    width: layout.width,
    height: layout.height,
    rgba,
    generation,
  });
  Ok(())
}
