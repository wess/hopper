use super::Owner;
use anyhow::Context;

#[derive(Clone, Copy, Debug)]
pub struct Status {
  pub state: super::super::VZVirtualMachineState,
  pub busy: bool,
  pub generation: u64,
  pub installer: bool,
  pub mac_ready: bool,
  pub started: bool,
  pub stop_requested: bool,
  pub network_connected: Option<bool>,
  pub sharing_devices: usize,
  pub audio_devices: usize,
}

impl Owner {
  pub fn inspect(&self, id: &str) -> anyhow::Result<Status> {
    let vm = self.machines.get(id).context("VZ machine is not owned")?;
    Ok(Status {
      state: super::super::state(vm),
      busy: self.pending.contains_key(id)
        || self.installations.contains_key(id)
        || vm.installing.get(),
      generation: vm.generation.get(),
      installer: vm.installer,
      mac_ready: vm.mac_ready.get(),
      started: vm.started.get(),
      stop_requested: vm.stop_requested.get(),
      sharing_devices: super::super::sharing::device_count(vm),
      audio_devices: vm.audio_devices,
      network_connected: match super::super::network::attachments(vm).as_slice() {
        [connected] => Some(*connected),
        _ => None,
      },
    })
  }
}
