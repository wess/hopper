//! native hardware compatibility; not authorization or captured guest contents.

use crate::{devices::virtio::pci, runtime::Boot};
use anyhow::Context;
use sha2::{Digest, Sha256};

/// Freeze this value from the owned prepared hardware before guest execution.
/// Bump the domain when the fixed platform/MMIO/firmware contract changes.
pub fn fingerprint(
  machine: &str,
  boot: &Boot,
  devices: &[Option<pci::Device>; 7],
  mut authorize: impl FnMut() -> anyhow::Result<()>,
) -> anyhow::Result<[u8; 32]> {
  authorize()?;
  let identity = model::native::machine_identity(machine).context("Invalid native VM identity")?;
  crate::runtime::validate(boot)?;
  let mut hash = Sha256::new();
  hash.update(b"hopper.native.configuration.v1");
  hash.update(identity);
  hash.update(boot.memory.to_le_bytes());
  hash.update(boot.cpus.to_le_bytes());
  hash.update((boot.firmware.len() as u64).to_le_bytes());
  for chunk in boot.firmware.chunks(65536) {
    authorize()?;
    hash.update(chunk);
    authorize()?;
  }
  for device in devices {
    authorize()?;
    hash.update([device.is_some() as u8]);
    if let Some(device) = device {
      hash.update(pci::snapshot::configuration::fingerprint(device));
    }
    authorize()?;
  }
  let configuration = hash.finalize().into();
  authorize()?;
  Ok(configuration)
}
