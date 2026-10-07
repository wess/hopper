use super::{Input, Kind};

pub fn read(input: &Input) -> [u8; 136] {
  let mut bytes = [0; 136];
  bytes[0] = input.select;
  bytes[1] = input.subselect;
  let data = &mut bytes[8..];
  let mut size = 0;
  match (input.select, input.subselect) {
    (1, 0) => {
      let name: &[u8] = match input.kind {
        Kind::Keyboard => b"Hopper Keyboard",
        Kind::Tablet => b"Hopper Pointer",
      };
      data[..name.len()].copy_from_slice(name);
      size = name.len();
    }
    (3, 0) => {
      for (index, value) in [6u16, 0x1af4, 0x1052, 1].into_iter().enumerate() {
        data[index * 2..index * 2 + 2].copy_from_slice(&value.to_le_bytes());
      }
      size = 8;
    }
    (0x10, 0) if input.kind == Kind::Tablet => {
      data[0] = 1;
      size = 1;
    }
    (0x11, 0) => {
      data[0] = 1;
      size = 1;
    }
    (0x11, 1) => {
      let codes = match input.kind {
        Kind::Keyboard => 1..=255,
        Kind::Tablet => 272..=274,
      };
      for code in codes {
        data[code / 8] |= 1 << (code % 8);
        size = code / 8 + 1;
      }
    }
    (0x11, 0x11) if input.kind == Kind::Keyboard => {
      data[0] = 7;
      size = 1;
    }
    (0x11, 2) if input.kind == Kind::Tablet => {
      data[1] = 1;
      size = 2;
    }
    (0x11, 3) if input.kind == Kind::Tablet => {
      data[0] = 3;
      size = 1;
    }
    (0x12, 0 | 1) if input.kind == Kind::Tablet => {
      data[4..8].copy_from_slice(&65535u32.to_le_bytes());
      size = 20;
    }
    _ => {}
  }
  bytes[2] = size as u8;
  bytes
}

pub fn write(input: &mut Input, offset: usize, width: usize, value: u32) {
  for (index, byte) in value.to_le_bytes().iter().take(width).enumerate() {
    match offset + index {
      0 => input.select = *byte,
      1 => input.subselect = *byte,
      _ => {}
    }
  }
}
