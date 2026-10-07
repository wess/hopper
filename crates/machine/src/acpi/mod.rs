//! ACPI 6.3 handoff for the native ARM platform.

mod aml;
mod arm;
mod pci;

use crate::platform::{self, Topology};
use anyhow::ensure;

pub const BASE: u64 = 0x40200000;
pub const SIZE: usize = 0x10000;

pub(super) fn put(bytes: &mut [u8], offset: usize, value: &[u8]) {
  bytes[offset..offset + value.len()].copy_from_slice(value);
}

fn checksum(bytes: &mut [u8], offset: usize) {
  bytes[offset] = 0;
  bytes[offset] = 0u8.wrapping_sub(bytes.iter().fold(0u8, |sum, byte| sum.wrapping_add(*byte)));
}

pub(super) fn table(signature: &[u8; 4], revision: u8, body: &[u8]) -> Vec<u8> {
  let mut bytes = vec![0; 36];
  put(&mut bytes, 0, signature);
  put(&mut bytes, 4, &(36u32 + body.len() as u32).to_le_bytes());
  bytes[8] = revision;
  put(&mut bytes, 10, b"HOPPER");
  put(&mut bytes, 16, b"HOPPER  ");
  put(&mut bytes, 24, &1u32.to_le_bytes());
  put(&mut bytes, 28, b"HPPR");
  put(&mut bytes, 32, &1u32.to_le_bytes());
  bytes.extend(body);
  checksum(&mut bytes, 9);
  bytes
}

fn append(blob: &mut Vec<u8>, bytes: &[u8]) -> u64 {
  blob.resize(blob.len().next_multiple_of(8), 0);
  let address = BASE + blob.len() as u64;
  blob.extend(bytes);
  address
}

pub fn bundle(topology: &Topology) -> anyhow::Result<Vec<u8>> {
  platform::validate(topology)?;
  let mut blob = vec![0; 36];
  let dsdt = append(&mut blob, &table(b"DSDT", 2, &aml::namespace(topology)));
  let mut pointers = Vec::new();
  for bytes in [
    arm::fadt(dsdt),
    arm::madt(topology),
    arm::timers(),
    pci::mcfg(),
  ] {
    pointers.extend(append(&mut blob, &bytes).to_le_bytes());
  }
  let xsdt = append(&mut blob, &table(b"XSDT", 1, &pointers));
  let rsdp = &mut blob[..36];
  put(rsdp, 0, b"RSD PTR ");
  put(rsdp, 9, b"HOPPER");
  rsdp[15] = 2;
  put(rsdp, 20, &36u32.to_le_bytes());
  put(rsdp, 24, &xsdt.to_le_bytes());
  checksum(&mut rsdp[..20], 8);
  checksum(rsdp, 32);
  ensure!(
    blob.len() <= SIZE,
    "ACPI tables exceed their reserved region"
  );
  blob.resize(SIZE, 0);
  Ok(blob)
}
