//! HPC1 reopens configuration only against a matching authorized function layout.

use super::Function;
use anyhow::ensure;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

pub const MAX: usize = 4300;

pub fn encode(function: &Function) -> anyhow::Result<Zeroizing<Vec<u8>>> {
  ensure!(function.bars.len() <= 6, "PCI record BAR count is invalid");
  let mut bytes = Zeroizing::new(Vec::from(b"HPC1"));
  for value in [
    function.capability,
    function.next_capability,
    function.msix.unwrap_or(0xffff),
  ] {
    bytes.extend_from_slice(&u16::try_from(value)?.to_le_bytes());
  }
  bytes.extend_from_slice(&function.config);
  bytes.push(function.bars.len() as u8);
  for bar in &function.bars {
    bytes.push(bar.slot as u8);
    bytes.extend_from_slice(&bar.size.to_le_bytes());
    bytes.push(bar.wide as u8);
    bytes.extend_from_slice(&bar.address.to_le_bytes());
    bytes.extend(bar.probe.map(u8::from));
  }
  let checksum = Sha256::digest(&*bytes);
  bytes.extend_from_slice(&checksum);
  ensure!(bytes.len() <= MAX, "PCI record exceeds its bound");
  Ok(bytes)
}

pub fn decode(bytes: &[u8], expected: &Function) -> anyhow::Result<Function> {
  ensure!(
    (4139..=MAX).contains(&bytes.len()),
    "PCI record size is invalid"
  );
  let end = bytes.len() - 32;
  ensure!(
    Sha256::digest(&bytes[..end])[..] == bytes[end..],
    "PCI record checksum is invalid"
  );
  ensure!(&bytes[..4] == b"HPC1", "PCI record format is unsupported");
  for (index, expected) in [
    expected.capability,
    expected.next_capability,
    expected.msix.unwrap_or(0xffff),
  ]
  .into_iter()
  .enumerate()
  {
    let offset = 4 + index * 2;
    ensure!(
      u16::from_le_bytes(bytes[offset..offset + 2].try_into()?) as usize == expected,
      "PCI record capability layout is incompatible"
    );
  }
  let config = &bytes[10..4106];
  let count = bytes[4106] as usize;
  ensure!(
    count <= 6 && count == expected.bars.len() && 4107 + count * 20 == end,
    "PCI record BAR layout is incompatible"
  );
  let mut reopened = expected.clone();
  for (index, bar) in reopened.bars.iter_mut().enumerate() {
    let offset = 4107 + index * 20;
    ensure!(
      bytes[offset] as usize == bar.slot
        && bytes[offset + 9] == bar.wide as u8
        && u64::from_le_bytes(bytes[offset + 1..offset + 9].try_into()?) == bar.size,
      "PCI record BAR geometry is incompatible"
    );
    let address = u64::from_le_bytes(bytes[offset + 10..offset + 18].try_into()?);
    let probe = [bytes[offset + 18], bytes[offset + 19]];
    ensure!(
      address.is_multiple_of(bar.size)
        && (bar.wide || address <= u32::MAX as u64)
        && probe.iter().all(|value| *value <= 1)
        && (bar.wide || probe[1] == 0),
      "PCI record BAR state is invalid"
    );
    bar.address = address;
    bar.probe = probe.map(|value| value == 1);
  }
  reopened.write(4, 1, config[4].into())?;
  reopened.write(5, 1, config[5].into())?;
  reopened.write(7, 1, (!config[7]).into())?;
  reopened.write(0x3c, 1, config[0x3c].into())?;
  reopened.interrupt_status(config[6] & 8 != 0);
  if let Some(offset) = reopened.msix {
    reopened.write(offset + 3, 1, config[offset + 3].into())?;
  }
  ensure!(
    reopened.config == config,
    "PCI record changes immutable or reserved configuration"
  );
  Ok(reopened)
}
