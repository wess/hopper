use super::{
  resource::{self, Rect, Resource},
  Display, Frame, EXHAUSTED, INVALID, LIMIT, MISSING,
};
use crate::dma::{self, Memory, Span};

pub(super) fn word(bytes: &[u8], offset: usize) -> u32 {
  u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn rect(bytes: &[u8]) -> Rect {
  Rect {
    x: word(bytes, 24),
    y: word(bytes, 28),
    width: word(bytes, 32),
    height: word(bytes, 36),
  }
}

fn bytes(memory: &impl Memory, spans: &[Span], length: usize) -> Result<Vec<u8>, u32> {
  let mut bytes = vec![0; length];
  dma::read(memory, spans, 0, &mut bytes).map_err(|_| INVALID)?;
  Ok(bytes)
}

pub(super) fn execute(
  display: &mut Display,
  memory: &impl Memory,
  spans: &[Span],
  command: u32,
  reply: &mut [u8],
) -> Result<(), u32> {
  match command {
    0x100 => {
      reply[32..36].copy_from_slice(&display.preferred.0.to_le_bytes());
      reply[36..40].copy_from_slice(&display.preferred.1.to_le_bytes());
      reply[40..44].copy_from_slice(&1u32.to_le_bytes());
    }
    0x101 => {
      let data = bytes(memory, spans, 40)?;
      let id = word(&data, 24);
      let format = word(&data, 28);
      let width = word(&data, 32);
      let height = word(&data, 36);
      if id == 0
        || display.resources.contains_key(&id)
        || !matches!(format, 1..=4 | 67 | 68 | 121 | 134)
      {
        return Err(INVALID);
      }
      let size = resource::size(width, height).ok_or(INVALID)?;
      if display.resources.len() >= 64 || size > LIMIT - display.allocated {
        return Err(EXHAUSTED);
      }
      let mut pixels = Vec::new();
      pixels.try_reserve_exact(size).map_err(|_| EXHAUSTED)?;
      pixels.resize(size, 0);
      display.resources.insert(
        id,
        Resource {
          width,
          height,
          format,
          pixels,
          backing: Vec::new(),
        },
      );
      display.allocated += size;
    }
    0x102 => {
      let data = bytes(memory, spans, 32)?;
      let id = word(&data, 24);
      let resource = display.resources.remove(&id).ok_or(MISSING)?;
      display.allocated -= resource.pixels.len();
      if display.scanout.is_some_and(|(selected, _)| selected == id) {
        display.scanout = None;
        display.frame = None;
        display.generation = display.generation.wrapping_add(1);
      }
    }
    0x103 => {
      let data = bytes(memory, spans, 48)?;
      if word(&data, 40) != 0 {
        return Err(0x1202);
      }
      let id = word(&data, 44);
      if id == 0 {
        display.scanout = None;
        display.frame = None;
        display.generation = display.generation.wrapping_add(1);
      } else {
        let area = rect(&data);
        let resource = display.resources.get(&id).ok_or(MISSING)?;
        if !resource::covered(resource, area) {
          return Err(INVALID);
        }
        // allocate before changing the selected resource, so a failed mode switch is recoverable.
        let size = area.width as usize * area.height as usize * 4;
        let mut rgba = Vec::new();
        rgba.try_reserve_exact(size).map_err(|_| EXHAUSTED)?;
        rgba.resize(size, 0);
        display.generation = display.generation.wrapping_add(1);
        display.frame = Some(Frame {
          width: area.width,
          height: area.height,
          rgba,
          generation: display.generation,
        });
        display.scanout = Some((id, area));
        flush(display, id, area)?;
      }
      super::linear::disable(&mut display.linear);
    }
    0x104 => {
      let data = bytes(memory, spans, 48)?;
      flush(display, word(&data, 40), rect(&data))?;
    }
    0x105 => {
      let data = bytes(memory, spans, 56)?;
      let area = rect(&data);
      let offset = u64::from_le_bytes(data[40..48].try_into().unwrap());
      let resource = display.resources.get_mut(&word(&data, 48)).ok_or(MISSING)?;
      if !resource::covered(resource, area) || resource.backing.is_empty() {
        return Err(INVALID);
      }
      let stride = resource.width as u64 * 4;
      let row = area.width as usize * 4;
      let end = offset
        .checked_add((area.height as u64 - 1) * stride)
        .and_then(|offset| offset.checked_add(row as u64))
        .ok_or(INVALID)?;
      if end > dma::length(&resource.backing) {
        return Err(INVALID);
      }
      for y in 0..area.height as usize {
        let target = ((area.y as usize + y) * resource.width as usize + area.x as usize) * 4;
        dma::read(
          memory,
          &resource.backing,
          offset + y as u64 * stride,
          &mut resource.pixels[target..target + row],
        )
        .map_err(|_| INVALID)?;
      }
    }
    0x106 => {
      let data = bytes(memory, spans, 32)?;
      let id = word(&data, 24);
      let count = word(&data, 28) as usize;
      if count == 0 || count > 4096 {
        return Err(INVALID);
      }
      let resource = display.resources.get_mut(&id).ok_or(MISSING)?;
      if !resource.backing.is_empty() {
        return Err(INVALID);
      }
      let data = bytes(memory, spans, 32 + count * 16)?;
      let mut backing = Vec::with_capacity(count);
      for entry in data[32..].as_chunks::<16>().0 {
        let address = u64::from_le_bytes(entry[..8].try_into().unwrap());
        let length = word(entry, 8);
        if length == 0 || !memory.contains(address, length as usize) {
          return Err(INVALID);
        }
        backing.push(Span { address, length });
      }
      resource.backing = backing;
    }
    0x107 => {
      let data = bytes(memory, spans, 32)?;
      display
        .resources
        .get_mut(&word(&data, 24))
        .ok_or(MISSING)?
        .backing
        .clear();
    }
    _ => return Err(0x1200),
  }
  Ok(())
}

fn flush(display: &mut Display, id: u32, area: Rect) -> Result<(), u32> {
  let resource = display.resources.get(&id).ok_or(MISSING)?;
  if !resource::covered(resource, area) {
    return Err(INVALID);
  }
  let Some((selected, scanout)) = display.scanout else {
    return Ok(());
  };
  if selected != id {
    return Ok(());
  }
  let x = area.x.max(scanout.x);
  let y = area.y.max(scanout.y);
  let right = (area.x + area.width).min(scanout.x + scanout.width);
  let bottom = (area.y + area.height).min(scanout.y + scanout.height);
  if x >= right || y >= bottom {
    return Ok(());
  }
  let frame = display.frame.as_mut().ok_or(INVALID)?;
  for row in y..bottom {
    for col in x..right {
      let source = (row as usize * resource.width as usize + col as usize) * 4;
      let target =
        ((row - scanout.y) as usize * scanout.width as usize + (col - scanout.x) as usize) * 4;
      frame.rgba[target..target + 4].copy_from_slice(&resource::rgba(
        resource.format,
        &resource.pixels[source..source + 4],
      ));
    }
  }
  display.generation = display.generation.wrapping_add(1);
  frame.generation = display.generation;
  Ok(())
}
