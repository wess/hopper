use super::{Actor, Machines};
use anyhow::ensure;
use model::GuestOs;
use std::sync::Arc;

pub use machine::vz::{
  queue::{channel, Client, Owner},
  Action,
};

#[derive(Clone)]
pub struct Service {
  manager: Machines,
  client: Client,
}

impl Service {
  pub fn new(manager: Machines, client: Client) -> Self {
    Self { manager, client }
  }

  pub async fn transition(&self, id: &str, actor: Actor, action: Action) -> anyhow::Result<()> {
    let machine = self.manager.machine(id, actor)?;
    ensure!(
      machine.guest != GuestOs::Windows,
      "Windows uses the native Hypervisor runtime"
    );
    ensure!(
      !self.manager.root.join("lima").join(id).try_exists()?,
      "Previous VM requires migration; its disk is preserved"
    );
    let generation = machine.agent_generation;
    let guest = machine.guest;
    let lease = self.manager.lock(id)?;
    let manager = self.manager.clone();
    let identity = id.to_owned();
    let check = Arc::new(move || {
      let _lease = &lease;
      let machine = manager.machine(&identity, actor)?;
      ensure!(
        !manager.root.join("lima").join(&identity).try_exists()?,
        "Previous VM requires migration; its disk is preserved"
      );
      ensure!(
        machine.guest == guest,
        "VM platform changed during the operation"
      );
      ensure!(
        actor != Actor::Agent || machine.agent_generation == generation,
        "VM agent policy has changed"
      );
      Ok(())
    });
    self.client.transition_checked(id, action, check).await
  }
}
