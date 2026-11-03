use super::{variables, Boot, Control, Mode, Stopped};
use crate::{devices::virtio::pci, hypervisor as hv, snapshot::storage::Prepared};
use anyhow::Context;

/// Trusted restore-stage ownership and original authorization, never agent-supplied file handles.
pub struct Disks {
  pub files: [Option<Prepared>; 7],
  pub authorize: Box<dyn FnMut() -> anyhow::Result<()>>,
}

impl Disks {
  pub(crate) fn check(&mut self, saved: &hv::state::group::Checkpoint) -> anyhow::Result<()> {
    (self.authorize)()?;
    for (slot, state) in saved.primary.devices.transports.iter().enumerate() {
      let required = state
        .as_ref()
        .is_some_and(|state| pci::snapshot::records::saved_kind(state) == 1);
      anyhow::ensure!(
        self.files[slot].is_some() == required,
        "Persistent recovery requires exactly the captured disk slots"
      );
    }
    for (slot, disk) in self.files.iter().enumerate() {
      if let Some(disk) = disk {
        let state = saved.primary.devices.transports[slot]
          .as_ref()
          .context("Prepared disk has no captured backend")?;
        pci::snapshot::check_prepared(state, disk, &mut self.authorize)?;
      }
    }
    Ok(())
  }
}

pub fn run_recovered(
  boot: Boot,
  devices: [Option<pci::Device>; 7],
  store: &mut variables::Variables,
  checkpoint: &hv::state::group::Checkpoint,
  mut disks: Disks,
  poll: impl FnMut(&mut [Option<pci::Device>; 7], &mut hv::Memory<'_>, Mode) -> anyhow::Result<Control>,
) -> anyhow::Result<Stopped> {
  (disks.authorize)()?;
  variables::matches(store, &checkpoint.primary.memory.nvram)?;
  (disks.authorize)()?;
  super::native::execute(
    boot,
    devices,
    Some(store),
    None,
    Some(checkpoint),
    Some(disks),
    poll,
  )
}

pub fn run_recovered_captured(
  boot: Boot,
  devices: [Option<pci::Device>; 7],
  store: &mut variables::Variables,
  checkpoint: &hv::state::group::Checkpoint,
  mut disks: Disks,
  group: std::sync::Arc<hv::state::group::Group>,
  poll: impl FnMut(&mut [Option<pci::Device>; 7], &mut hv::Memory<'_>, Mode) -> anyhow::Result<Control>,
) -> anyhow::Result<Stopped> {
  (disks.authorize)()?;
  variables::matches(store, &checkpoint.primary.memory.nvram)?;
  (disks.authorize)()?;
  super::native::execute(
    boot,
    devices,
    Some(store),
    Some(group),
    Some(checkpoint),
    Some(disks),
    poll,
  )
}
