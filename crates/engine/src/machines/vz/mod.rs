mod files;
mod prepare;

use super::{Actor, Machines};
use anyhow::ensure;
use model::GuestOs;
pub use prepare::{Admission, Prepared, Stage};
use std::sync::Arc;

pub use machine::vz::{
  queue::{channel, Client, Owner},
  Action, MainThreadMarker, VZVirtualMachineState as State,
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
    let (_, check) = self.scope(id, actor)?;
    self.client.transition_checked(id, action, check).await
  }

  pub async fn prepare_linux(
    &self,
    id: &str,
    actor: Actor,
    stage: Stage,
  ) -> anyhow::Result<Prepared> {
    let (machine, check) = self.scope(id, actor)?;
    ensure!(
      machine.guest == GuestOs::Linux,
      "Linux admission requires a Linux VM"
    );
    let manager = self.manager.clone();
    let client = self.client.clone();
    tokio::task::spawn_blocking(move || prepare::prepare(manager, machine, check, client, stage))
      .await?
  }

  fn scope(
    &self,
    id: &str,
    actor: Actor,
  ) -> anyhow::Result<(model::Machine, machine::vz::queue::Check)> {
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
    Ok((machine, check))
  }
}
