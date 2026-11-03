//! software/device state and native checkpoint records; runtime/agent integration is separate.

use crate::{debug, psci::power};
use anyhow::ensure;
pub mod actions;
pub mod configuration;
pub mod selection;
pub mod wire;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub mod checkpoint;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub mod generation;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub mod manifest;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub mod native;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub mod package;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub mod publication;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub mod storage;

pub struct Software {
  power: power::snapshot::Snapshot,
  debug: Vec<debug::State>,
}

impl Software {
  pub fn capture(power: &power::Power, debug: &[debug::State]) -> anyhow::Result<Self> {
    let power = power::snapshot::capture(power)?;
    ensure!(
      debug.len() == power.count(),
      "CPU debug state does not match the topology"
    );
    Ok(Self {
      power,
      debug: debug.to_vec(),
    })
  }

  pub fn power(&self) -> &power::snapshot::Snapshot {
    &self.power
  }

  pub fn debug(&self, index: usize) -> anyhow::Result<debug::State> {
    self
      .debug
      .get(index)
      .copied()
      .ok_or_else(|| anyhow::anyhow!("Unknown captured CPU debug state"))
  }

  /// Before any reconstructed CPU executes; all checks precede software mutation.
  pub fn restore(
    &self,
    power: &mut power::Power,
    debug: &mut [debug::State],
  ) -> anyhow::Result<()> {
    ensure!(
      debug.len() == self.debug.len(),
      "CPU debug state does not match the topology"
    );
    power::snapshot::restore(power, &self.power)?;
    debug.copy_from_slice(&self.debug);
    Ok(())
  }
}
