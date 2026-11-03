//! HGF1 streams resource pixels, published frames and independent active linear layout.

use super::super::{linear, resource, Display, Frame, LIMIT};
use super::State;
use anyhow::ensure;
use sha2::{Digest, Sha256};
use std::{io::Write, sync::Arc};
mod read;
mod stream;
pub use read::decode;

pub const MAX: u64 = 400 * 1024 * 1024;

fn frame_size(frame: &Frame) -> anyhow::Result<usize> {
  let size = resource::size(frame.width, frame.height)
    .ok_or_else(|| anyhow::anyhow!("GPU frame dimensions are invalid"))?;
  ensure!(size == frame.rgba.len(), "GPU frame byte count is invalid");
  Ok(size)
}

fn validate(display: &Display) -> anyhow::Result<()> {
  ensure!(
    resource::size(display.preferred.0, display.preferred.1).is_some()
      && display.resources.len() <= 64,
    "GPU record display dimensions are invalid"
  );
  let mut allocated = 0;
  for (id, resource) in &display.resources {
    let size = resource::size(resource.width, resource.height)
      .ok_or_else(|| anyhow::anyhow!("GPU resource dimensions are invalid"))?;
    ensure!(
      *id != 0
        && matches!(resource.format, 1..=4 | 67 | 68 | 121 | 134)
        && resource.pixels.len() == size
        && size <= LIMIT - allocated
        && resource.backing.len() <= 4096,
      "GPU record resource is invalid"
    );
    allocated += size;
    ensure!(
      resource
        .backing
        .iter()
        .all(|span| span.length != 0 && span.address.checked_add(span.length as u64).is_some()),
      "GPU record backing is invalid"
    );
  }
  ensure!(
    allocated == display.allocated,
    "GPU record resource allocation is inconsistent"
  );
  match (display.scanout, &display.frame) {
    (None, None) => {}
    (Some((id, rect)), Some(frame)) => {
      let resource = display
        .resources
        .get(&id)
        .ok_or_else(|| anyhow::anyhow!("GPU scanout resource is missing"))?;
      ensure!(
        resource::covered(resource, rect)
          && frame.width == rect.width
          && frame.height == rect.height
          && frame.generation == display.generation,
        "GPU scanout frame is inconsistent"
      );
      frame_size(frame)?;
    }
    _ => anyhow::bail!("GPU scanout/frame presence is inconsistent"),
  }
  ensure!(
    display.cursor.hot_x < 64
      && display.cursor.hot_y < 64
      && display
        .cursor
        .rgba
        .as_ref()
        .is_none_or(|pixels| pixels.len() == 16384),
    "GPU cursor state is invalid"
  );
  let active = &display.linear;
  ensure!(
    (active.registers[5] == 1) == active.layout.is_some() && active.registers[5] <= 1,
    "GPU linear enable state is inconsistent"
  );
  if let Some(layout) = active.layout {
    ensure!(
      resource::size(layout.width, layout.height).is_some()
        && layout.stride >= layout.width
        && layout.stride <= 8192
        && layout.stride as usize * layout.height as usize * 4 <= LIMIT
        && layout.address.is_multiple_of(4096)
        && layout
          .address
          .checked_add(u64::from(layout.stride) * u64::from(layout.height) * 4)
          .is_some(),
      "GPU active linear layout is invalid"
    );
    if let Some(frame) = &active.frame {
      ensure!(
        frame.width == layout.width && frame.height == layout.height,
        "GPU linear frame dimensions mismatch"
      );
      frame_size(frame)?;
    }
  } else {
    ensure!(
      active.frame.is_none(),
      "Disabled linear display retains a frame"
    );
  }
  Ok(())
}

pub fn encode(
  saved: &State,
  output: &mut impl Write,
  authorize: impl FnMut() -> anyhow::Result<()>,
) -> anyhow::Result<()> {
  validate(&saved.0)?;
  let mut writer = stream::Writer {
    output,
    authorize,
    digest: Sha256::new(),
  };
  writer.data(b"HGF1")?;
  let display = &saved.0;
  writer.word(display.preferred.0)?;
  writer.word(display.preferred.1)?;
  writer.number(display.generation)?;
  writer.word(display.resources.len() as u32)?;
  for (id, resource) in &display.resources {
    for value in [*id, resource.width, resource.height, resource.format] {
      writer.word(value)?;
    }
    writer.data(&resource.pixels)?;
    writer.word(resource.backing.len() as u32)?;
    let mut backing = Vec::with_capacity(resource.backing.len() * 12);
    for span in resource.backing.iter() {
      backing.extend_from_slice(&span.address.to_le_bytes());
      backing.extend_from_slice(&span.length.to_le_bytes());
    }
    writer.data(&backing)?;
  }
  writer.byte(display.scanout.is_some() as u8)?;
  if let Some((id, rect)) = display.scanout {
    for value in [id, rect.x, rect.y, rect.width, rect.height] {
      writer.word(value)?;
    }
  }
  write_frame(&mut writer, &display.frame)?;
  for value in [
    display.cursor.x,
    display.cursor.y,
    display.cursor.hot_x,
    display.cursor.hot_y,
  ] {
    writer.word(value)?;
  }
  writer.number(display.cursor.generation)?;
  writer.byte(display.cursor.rgba.is_some() as u8)?;
  if let Some(pixels) = &display.cursor.rgba {
    writer.data(pixels)?;
  }
  for value in display.linear.registers {
    writer.word(value)?;
  }
  writer.byte(display.linear.layout.is_some() as u8)?;
  if let Some(layout) = display.linear.layout {
    writer.number(layout.address)?;
    writer.word(layout.width)?;
    writer.word(layout.height)?;
    writer.word(layout.stride)?;
  }
  write_frame(&mut writer, &display.linear.frame)?;
  writer.output.write_all(&writer.digest.finalize())?;
  (writer.authorize)()
}

fn write_frame<W: Write, A: FnMut() -> anyhow::Result<()>>(
  writer: &mut stream::Writer<'_, W, A>,
  frame: &Option<Arc<Frame>>,
) -> anyhow::Result<()> {
  writer.byte(frame.is_some() as u8)?;
  if let Some(frame) = frame {
    writer.word(frame.width)?;
    writer.word(frame.height)?;
    writer.number(frame.generation)?;
    writer.data(&frame.rgba)?;
  }
  Ok(())
}
