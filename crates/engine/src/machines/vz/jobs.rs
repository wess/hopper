use super::{Service, State};
use crate::machines::Actor;
use anyhow::{ensure, Context};
use model::{
  guest::wire::{Action, Outcome},
  GuestOs, MachineRuntime,
};
use std::sync::Arc;

impl Service {
  pub async fn guest_request(&self, id: &str, action: Action) -> anyhow::Result<Outcome> {
    let policy = self.manager.machine(id, Actor::Agent)?.agent_generation;
    self.guest_request_scoped(id, policy, action).await
  }

  pub async fn guest_request_scoped(
    &self,
    id: &str,
    policy: u64,
    action: Action,
  ) -> anyhow::Result<Outcome> {
    let original = self.manager.machine(id, Actor::Agent)?;
    ensure!(
      original.agent_generation == policy,
      "Original guest command policy changed"
    );
    ensure!(
      original.guest == GuestOs::Linux && original.runtime == Some(MachineRuntime::Virtualization),
      "Guest commands require an owned native Linux VM"
    );
    let manager = self.manager.clone();
    let identity = id.to_owned();
    let session = Arc::new(move || {
      let current = manager.machine(&identity, Actor::Agent)?;
      ensure!(
        current.guest == GuestOs::Linux
          && current.runtime == Some(MachineRuntime::Virtualization)
          && current.agent_generation == policy
          && !manager.root.join("lima").join(&identity).try_exists()?,
        "Original guest command policy or runtime changed"
      );
      Ok(())
    });
    let lease = self.manager.guard(id, ".commands")?;
    let original_session = session.clone();
    let check = Arc::new(move || {
      let _lease = &lease;
      original_session()
    });
    let status = self
      .client
      .status_checked(id, check.clone())
      .await?
      .context("Guest runtime is not owned")?;
    ensure!(
      status.state == State::Running && !status.busy && !status.installer,
      "Guest commands require running installed hardware"
    );
    self
      .client
      .guest_request(id, status.generation, policy, action, (session, check))
      .await
  }
}
