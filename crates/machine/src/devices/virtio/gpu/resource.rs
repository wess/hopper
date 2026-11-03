use crate::dma::Span;
use std::sync::Arc;

#[derive(Clone)]
pub(super) struct Resource {
  pub width: u32,
  pub height: u32,
  pub format: u32,
  pub pixels: Arc<Vec<u8>>,
  pub backing: Arc<Vec<Span>>,
}

fn copy(bytes: &[u8]) -> Result<Vec<u8>, u32> {
  let mut copied = Vec::new();
  copied
    .try_reserve_exact(bytes.len())
    .map_err(|_| super::EXHAUSTED)?;
  copied.extend_from_slice(bytes);
  Ok(copied)
}

pub(super) fn writable(pixels: &mut Arc<Vec<u8>>) -> Result<&mut Vec<u8>, u32> {
  if Arc::get_mut(pixels).is_none() {
    *pixels = Arc::new(copy(pixels)?);
  }
  Arc::get_mut(pixels).ok_or(super::EXHAUSTED)
}

pub(super) fn writable_frame(frame: &mut Arc<super::Frame>) -> Result<&mut super::Frame, u32> {
  if Arc::get_mut(frame).is_none() {
    *frame = Arc::new(super::Frame {
      width: frame.width,
      height: frame.height,
      rgba: copy(&frame.rgba)?,
      generation: frame.generation,
    });
  }
  Arc::get_mut(frame).ok_or(super::EXHAUSTED)
}

#[derive(Clone, Copy)]
pub(super) struct Rect {
  pub x: u32,
  pub y: u32,
  pub width: u32,
  pub height: u32,
}

pub(super) fn size(width: u32, height: u32) -> Option<usize> {
  (width > 0 && height > 0 && width <= 8192 && height <= 8192)
    .then(|| width as usize * height as usize * 4)
    .filter(|size| *size <= super::LIMIT)
}

pub(super) fn covered(resource: &Resource, rect: Rect) -> bool {
  rect.width > 0
    && rect.height > 0
    && rect
      .x
      .checked_add(rect.width)
      .is_some_and(|x| x <= resource.width)
    && rect
      .y
      .checked_add(rect.height)
      .is_some_and(|y| y <= resource.height)
}

pub(super) fn rgba(format: u32, pixel: &[u8]) -> [u8; 4] {
  match format {
    1 => [pixel[2], pixel[1], pixel[0], pixel[3]],
    2 => [pixel[2], pixel[1], pixel[0], 255],
    3 => [pixel[1], pixel[2], pixel[3], pixel[0]],
    4 => [pixel[1], pixel[2], pixel[3], 255],
    67 => [pixel[0], pixel[1], pixel[2], pixel[3]],
    68 => [pixel[3], pixel[2], pixel[1], 255],
    121 => [pixel[3], pixel[2], pixel[1], pixel[0]],
    134 => [pixel[0], pixel[1], pixel[2], 255],
    _ => unreachable!(),
  }
}
