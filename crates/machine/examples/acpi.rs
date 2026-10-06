use anyhow::Context;
use machine::{acpi, platform::Topology};
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
  let out = PathBuf::from(
    std::env::args()
      .nth(1)
      .context("Provide an ACPI output directory")?,
  );
  let cpus = std::env::args()
    .nth(2)
    .map(|value| value.parse())
    .transpose()?
    .unwrap_or(1);
  let blob = acpi::bundle(&Topology {
    memory: 0x10000000,
    cpus,
    distributor_size: 0x10000,
    redistributor_size: 0x2000000,
  })?;
  std::fs::create_dir_all(&out)?;
  std::fs::write(out.join("handoff.bin"), &blob)?;
  let xsdt = u64::from_le_bytes(blob[24..32].try_into()?) - acpi::BASE;
  let mut addresses = vec![xsdt as usize];
  let end = xsdt as usize
    + u32::from_le_bytes(blob[xsdt as usize + 4..xsdt as usize + 8].try_into()?) as usize;
  for pointer in blob[xsdt as usize + 36..end].as_chunks::<8>().0 {
    addresses.push((u64::from_le_bytes(*pointer) - acpi::BASE) as usize);
  }
  let fadt = addresses[1];
  addresses
    .push((u64::from_le_bytes(blob[fadt + 140..fadt + 148].try_into()?) - acpi::BASE) as usize);
  for start in addresses {
    let size = u32::from_le_bytes(blob[start + 4..start + 8].try_into()?) as usize;
    let signature = std::str::from_utf8(&blob[start..start + 4])?.to_lowercase();
    std::fs::write(
      out.join(format!("{signature}.dat")),
      &blob[start..start + size],
    )?;
  }
  Ok(())
}
