mod commands;
mod cursor;
pub mod linear;
mod resource;

use super::queue::Chain;
use crate::dma::{self, Memory};
use anyhow::ensure;
use std::collections::BTreeMap;

const INVALID: u32 = 0x1205;
const MISSING: u32 = 0x1203;
const EXHAUSTED: u32 = 0x1201;
const LIMIT: usize = 128 * 1024 * 1024;

pub struct Display {
  resources: BTreeMap<u32, resource::Resource>,
  allocated: usize,
  preferred: (u32, u32),
  scanout: Option<(u32, resource::Rect)>,
  frame: Option<Frame>,
  generation: u64,
  cursor: cursor::Cursor,
  linear: linear::Linear,
}

pub use cursor::{execute as execute_cursor, Cursor};

pub fn cursor(display: &Display) -> &Cursor {
  &display.cursor
}

pub struct Frame {
  pub width: u32,
  pub height: u32,
  pub rgba: Vec<u8>,
  pub generation: u64,
}

pub fn create(width: u32, height: u32) -> anyhow::Result<Display> {
  ensure!(
    resource::size(width, height).is_some(),
    "Invalid display dimensions"
  );
  Ok(Display {
    resources: BTreeMap::new(),
    allocated: 0,
    preferred: (width, height),
    scanout: None,
    frame: None,
    generation: 0,
    cursor: Cursor::default(),
    linear: linear::Linear::default(),
  })
}

pub fn frame(display: &Display) -> Option<&Frame> {
  if linear::active(&display.linear) {
    display.linear.frame.as_ref()
  } else {
    display.frame.as_ref()
  }
}

pub fn refresh(display: &mut Display, memory: &impl Memory) -> anyhow::Result<()> {
  linear::refresh(&mut display.linear, memory)
}

pub fn linear_read(display: &Display, offset: u64) -> u32 {
  linear::read(&display.linear, offset)
}

pub fn linear_write(
  display: &mut Display,
  memory: &impl Memory,
  offset: u64,
  value: u32,
) -> anyhow::Result<()> {
  ensure!(
    offset < 32 && offset.is_multiple_of(4),
    "Invalid linear framebuffer register"
  );
  linear::write(&mut display.linear, memory, offset, value)
}

pub fn reset(display: &mut Display) {
  display.resources.clear();
  display.allocated = 0;
  display.scanout = None;
  display.frame = None;
  display.cursor = Cursor::default();
  display.generation = display.generation.wrapping_add(1);
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
  ensure!(dma::length(&readable) >= 24, "Truncated GPU command header");
  let mut header = [0; 24];
  dma::read(memory, &readable, 0, &mut header)?;
  let command = commands::word(&header, 0);
  let capacity = if command == 0x100 { 408 } else { 24 };
  ensure!(
    dma::length(&writable) >= capacity,
    "Truncated GPU response buffer"
  );
  let mut reply = vec![0; capacity as usize];
  let flags = commands::word(&header, 4);
  let result = if flags & !1 != 0 || commands::word(&header, 16) != 0 {
    Err(INVALID)
  } else {
    commands::execute(display, memory, &readable, command, &mut reply)
  };
  let status = match result {
    Ok(()) => {
      if command == 0x100 {
        0x1101u32
      } else {
        0x1100u32
      }
    }
    Err(status) => {
      reply.truncate(24);
      status
    }
  };
  reply[..4].copy_from_slice(&status.to_le_bytes());
  if flags & 1 != 0 {
    reply[4..16].copy_from_slice(&header[4..16]);
    reply[4..8].copy_from_slice(&1u32.to_le_bytes());
  }
  dma::write(memory, &writable, 0, &reply)?;
  Ok(reply.len() as u32)
}
