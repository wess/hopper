//! HUR1 preserves PL011 registers and bounded pending receive bytes.

use super::Console;
use anyhow::ensure;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

pub const MAX: usize = 69;

fn validate(console: &Console) -> anyhow::Result<()> {
  ensure!(
    console.control & !0xff87 == 0
      && console.line <= 0xff
      && console.integer_baud <= 0xffff
      && console.fractional_baud <= 0x3f
      && console.receive.len() <= 16,
    "UART record state is invalid"
  );
  Ok(())
}

pub fn encode(console: &Console) -> anyhow::Result<Zeroizing<Vec<u8>>> {
  validate(console)?;
  let mut bytes = Zeroizing::new(Vec::from(b"HUR1"));
  for register in [
    console.control,
    console.line,
    console.integer_baud,
    console.fractional_baud,
  ] {
    bytes.extend_from_slice(&register.to_le_bytes());
  }
  bytes.push(console.receive.len() as u8);
  bytes.extend(console.receive.iter().copied());
  let checksum = Sha256::digest(&*bytes);
  bytes.extend_from_slice(&checksum);
  Ok(bytes)
}

pub fn decode(bytes: &[u8]) -> anyhow::Result<Console> {
  ensure!(
    (53..=MAX).contains(&bytes.len()),
    "UART record size is invalid"
  );
  ensure!(&bytes[..4] == b"HUR1", "UART record format is unsupported");
  let end = bytes.len() - 32;
  ensure!(
    Sha256::digest(&bytes[..end])[..] == bytes[end..],
    "UART record checksum is invalid"
  );
  let count = bytes[20] as usize;
  ensure!(
    count <= 16 && 21 + count == end,
    "UART record receive length is invalid"
  );
  let number = |offset| -> anyhow::Result<u32> {
    Ok(u32::from_le_bytes(bytes[offset..offset + 4].try_into()?))
  };
  let console = Console {
    control: number(4)?,
    line: number(8)?,
    integer_baud: number(12)?,
    fractional_baud: number(16)?,
    receive: bytes[21..end].iter().copied().collect(),
  };
  validate(&console)?;
  Ok(console)
}
