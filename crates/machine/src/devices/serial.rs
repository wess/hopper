//! Polled PL011 console for early firmware boot diagnostics.

use std::collections::VecDeque;

pub struct Console {
  pub control: u32,
  line: u32,
  integer_baud: u32,
  fractional_baud: u32,
  receive: VecDeque<u8>,
}

impl Default for Console {
  fn default() -> Self {
    Self {
      control: 0x300,
      line: 0,
      integer_baud: 0,
      fractional_baud: 0,
      receive: VecDeque::new(),
    }
  }
}

pub fn read(console: &mut Console, offset: u64) -> u32 {
  match offset {
    0 => console.receive.pop_front().unwrap_or(0).into(),
    0x18 => {
      0x80
        | if console.receive.is_empty() { 0x10 } else { 0 }
        | if console.receive.len() >= capacity(console) {
          0x40
        } else {
          0
        }
    }
    0x24 => console.integer_baud,
    0x28 => console.fractional_baud,
    0x2c => console.line,
    0x30 => console.control,
    0xfe0 => 0x11,
    0xfe4 => 0x10,
    0xfe8 => 0x14,
    0xff0 => 0x0d,
    0xff4 => 0xf0,
    0xff8 => 0x05,
    0xffc => 0xb1,
    _ => 0,
  }
}

pub fn write(console: &mut Console, offset: u64, value: u32) -> Option<u8> {
  match offset {
    0 if console.control & 0x101 == 0x101 => return Some(value as u8),
    0x24 => console.integer_baud = value & 0xffff,
    0x28 => console.fractional_baud = value & 0x3f,
    0x2c => console.line = value & 0xff,
    0x30 => console.control = value & 0xff87,
    _ => {}
  }
  None
}

pub fn receive(console: &mut Console, byte: u8) -> bool {
  if console.control & 0x201 != 0x201 || console.receive.len() >= capacity(console) {
    return false;
  }
  console.receive.push_back(byte);
  true
}

fn capacity(console: &Console) -> usize {
  if console.line & 0x10 != 0 {
    16
  } else {
    1
  }
}
