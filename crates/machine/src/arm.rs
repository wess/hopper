//! Decode the traps used by the device bus and firmware services.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Access {
  pub register: u8,
  pub bytes: u8,
  pub write: bool,
  pub sign_extend: bool,
  pub wide_register: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trap {
  Hypercall(u16),
  SecureCall(u16),
  DataAbort(Option<Access>),
  Other(u8),
}

pub fn decode(syndrome: u64) -> Trap {
  let class = ((syndrome >> 26) & 0x3f) as u8;
  match class {
    0x16 => Trap::Hypercall(syndrome as u16),
    0x17 => Trap::SecureCall(syndrome as u16),
    0x24 => Trap::DataAbort((syndrome & (1 << 24) != 0).then_some(Access {
      register: ((syndrome >> 16) & 31) as u8,
      bytes: 1 << ((syndrome >> 22) & 3),
      write: syndrome & (1 << 6) != 0,
      sign_extend: syndrome & (1 << 21) != 0,
      wide_register: syndrome & (1 << 15) != 0,
    })),
    _ => Trap::Other(class),
  }
}
