use anyhow::{ensure, Context};
use gpui::RenderImage;
use std::sync::Arc;

pub(super) fn render(mut frame: host::MachineFrame) -> anyhow::Result<Arc<RenderImage>> {
  ensure!(
    (1..=4096).contains(&frame.width) && (1..=4096).contains(&frame.height),
    "Invalid guest display size"
  );
  let size = (frame.width as usize)
    .checked_mul(frame.height as usize)
    .and_then(|size| size.checked_mul(4))
    .context("Guest display size overflow")?;
  ensure!(
    frame.rgba.len() == size && size <= 64 * 1024 * 1024,
    "Invalid guest display frame"
  );
  // gpui uploads BGRA; guest captures arrive as RGBA.
  for pixel in frame.rgba.as_chunks_mut::<4>().0 {
    pixel.swap(0, 2);
  }
  let pixels = image::RgbaImage::from_raw(frame.width, frame.height, frame.rgba)
    .context("Invalid guest display pixels")?;
  Ok(Arc::new(RenderImage::new(vec![image::Frame::new(pixels)])))
}
