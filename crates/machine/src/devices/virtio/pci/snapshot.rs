//! backend and transport state; durable storage publication and full runtime recovery are pending.

use super::{backend::Backend, msix, queue, Device, QueueState};
use crate::{devices::pci::Function, dma::Memory};
use anyhow::ensure;
mod backend;
pub mod backendwire;
pub mod configuration;
pub mod gpuwire;
pub mod records;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub mod storagewire;
pub mod wire;

pub struct State {
  backend: Option<backend::State>,
  kind: u8,
  offered: u64,
  pci: Function,
  common: [u8; 56],
  driver_features: u64,
  unsupported: bool,
  queues: Vec<SavedQueue>,
  isr: u8,
  window: [u8; 20],
  window_offset: usize,
  completed: u64,
  fault: Option<String>,
  msix: msix::State,
}

struct SavedQueue {
  registers: [u8; 32],
  queue: Option<queue::snapshot::State>,
}

pub fn capture(device: &Device) -> anyhow::Result<State> {
  let queues = device
    .queues
    .iter()
    .map(|state| {
      Ok(SavedQueue {
        registers: state.registers,
        queue: state
          .queue
          .as_ref()
          .map(queue::snapshot::capture)
          .transpose()?,
      })
    })
    .collect::<anyhow::Result<Vec<_>>>()?;
  Ok(State {
    backend: backend::capture(&device.backend)?,
    kind: kind(&device.backend),
    offered: super::features(device),
    pci: device.pci.clone(),
    common: device.common,
    driver_features: device.driver_features,
    unsupported: device.unsupported,
    queues,
    isr: device.isr,
    window: device.window,
    window_offset: device.window_offset,
    completed: device.completed,
    fault: device.fault.clone(),
    msix: device.msix.clone(),
  })
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub fn prepare_disk(
  state: &State,
  directory: &std::fs::File,
  authorize: impl FnMut() -> anyhow::Result<()>,
) -> anyhow::Result<crate::snapshot::storage::Prepared> {
  let Some(backend::State::Disk(disk)) = &state.backend else {
    anyhow::bail!("Captured backend is not a writable system disk");
  };
  crate::devices::virtio::block::snapshot::prepare(disk, directory, authorize)
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub fn reopen_prepared_disk(
  state: &State,
  directory: &std::fs::File,
  authorize: impl FnMut() -> anyhow::Result<()>,
) -> anyhow::Result<crate::snapshot::storage::Prepared> {
  let Some(backend::State::Disk(disk)) = &state.backend else {
    anyhow::bail!("Captured backend is not a writable system disk");
  };
  crate::devices::virtio::block::snapshot::reopen_prepared(disk, directory, authorize)
}

/// Complete reconstruction for supported backends before execution.
/// Does not restore host input leases, transport authorization or guest job sessions.
pub fn restore(state: &State, device: &mut Device, memory: &impl Memory) -> anyhow::Result<()> {
  let saved = state
    .backend
    .as_ref()
    .ok_or_else(|| anyhow::anyhow!("Device backend recovery is unavailable"))?;
  let backend = backend::restore(saved, &device.backend, memory)?;
  restore_transport(state, device, memory)?;
  device.backend = backend;
  Ok(())
}

/// All validation precedes mutation. The caller must restore the matching backend
/// separately; this cannot authorize disk/media reuse or reconstruct a complete device.
pub fn restore_transport(
  state: &State,
  device: &mut Device,
  memory: &impl Memory,
) -> anyhow::Result<()> {
  ensure!(
    state.kind == kind(&device.backend)
      && state.offered == super::features(device)
      && state.queues.len() == device.queues.len()
      && state.window_offset == device.window_offset
      && msix::count(&state.msix) == msix::count(&device.msix)
      && state.pci.compatible(&device.pci),
    "Captured Virtio transport does not match the device layout"
  );
  let queues = state
    .queues
    .iter()
    .map(|saved| {
      Ok(QueueState {
        registers: saved.registers,
        queue: saved
          .queue
          .as_ref()
          .map(|queue| queue::snapshot::restore(queue, memory))
          .transpose()?,
      })
    })
    .collect::<anyhow::Result<Vec<_>>>()?;
  device.pci = state.pci.clone();
  device.common = state.common;
  device.driver_features = state.driver_features;
  device.unsupported = state.unsupported;
  device.queues = queues;
  device.isr = state.isr;
  device.window = state.window;
  device.window_offset = state.window_offset;
  device.completed = state.completed;
  device.fault.clone_from(&state.fault);
  device.msix = state.msix.clone();
  Ok(())
}

fn kind(backend: &Backend) -> u8 {
  match backend {
    Backend::Disk(_) => 1,
    Backend::Gpu(_) => 2,
    Backend::Input(_) => 3,
    Backend::Optical(_) => 4,
    Backend::Console(_) => 5,
  }
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub(crate) fn check_prepared(
  state: &State,
  prepared: &crate::snapshot::storage::Prepared,
  authorize: impl FnMut() -> anyhow::Result<()>,
) -> anyhow::Result<()> {
  let Some(backend::State::Disk(disk)) = &state.backend else {
    anyhow::bail!("Persistent restore requires a captured disk backend");
  };
  crate::devices::virtio::block::snapshot::check_prepared(disk, prepared, authorize)
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub(crate) fn restore_prepared(
  state: &State,
  device: &mut Device,
  memory: &impl Memory,
  prepared: crate::snapshot::storage::Prepared,
  authorize: impl FnMut() -> anyhow::Result<()>,
) -> anyhow::Result<()> {
  let (Some(backend::State::Disk(saved)), Backend::Disk(target)) =
    (&state.backend, &device.backend)
  else {
    anyhow::bail!("Persistent restore disk backend mismatch");
  };
  let disk = crate::devices::virtio::block::snapshot::adopt(saved, target, prepared, authorize)?;
  restore_transport(state, device, memory)?;
  device.backend = Backend::Disk(disk);
  Ok(())
}
