use crate::{Host, MachineActor, VirtualMacLaunch, VirtualMacPhase};

impl Host {
  fn virtual_mac_service(&self) -> anyhow::Result<::engine::machines::vz::Service> {
    self
      .virtual_machines
      .lock()
      .map_err(|_| anyhow::anyhow!("VZ service lock failed"))?
      .clone()
      .ok_or_else(|| anyhow::anyhow!("VZ ownership is not connected"))
  }

  pub async fn prepare_virtual_mac(
    &self,
    id: &str,
    actor: MachineActor,
    progress: tokio::sync::watch::Sender<VirtualMacPhase>,
  ) -> anyhow::Result<VirtualMacLaunch> {
    self
      .virtual_mac_service()?
      .prepare_mac(id, actor, progress)
      .await
  }

  pub fn cancel_virtual_mac(&self, id: &str, actor: MachineActor) -> anyhow::Result<()> {
    self.virtual_mac_service()?.cancel_mac(id, actor)
  }
}
