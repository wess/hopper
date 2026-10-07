pub(crate) mod files;
mod identity;
mod prepare;

use super::{Actor, Machines};
use anyhow::ensure;
use model::GuestOs;
pub use prepare::{Admission, Prepared, Stage};
use std::sync::Arc;

pub use machine::vz::{
  queue::{channel, Client, Owner, Status},
  Action, Display, MainThreadMarker, VZVirtualMachineState as State,
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

  pub async fn status(&self, id: &str, actor: Actor) -> anyhow::Result<Option<Status>> {
    let machine = self.manager.machine(id, actor)?;
    ensure!(
      machine.guest != GuestOs::Windows,
      "Windows uses the native Hypervisor runtime"
    );
    if !self.manager.native_vz(id)? {
      return Ok(None);
    }
    let manager = self.manager.clone();
    let identity = id.to_owned();
    let check = Arc::new(move || {
      let current = manager.machine(&identity, actor)?;
      ensure!(
        current.guest == machine.guest && current.runtime == machine.runtime,
        "VM platform or runtime changed during the operation"
      );
      ensure!(
        actor != Actor::Agent || current.agent_generation == machine.agent_generation,
        "VM agent policy has changed"
      );
      Ok(())
    });
    self.client.status_checked(id, check).await
  }

  pub async fn prepare_linux(
    &self,
    id: &str,
    actor: Actor,
    stage: Stage,
  ) -> anyhow::Result<Prepared> {
    let (mut machine, check) = self.scope(id, actor)?;
    ensure!(
      machine.guest == GuestOs::Linux,
      "Linux admission requires a Linux VM"
    );
    crate::machines::config::validate_resources(&machine)?;
    let launch = stage == Stage::Launch;
    let stage = if launch {
      ensure!(
        machine.profile == "ubuntu"
          && machine.runtime == Some(model::MachineRuntime::Virtualization),
        "Automatic launch requires native Ubuntu"
      );
      match self.installation(id, actor)? {
        Some(
          super::linux::progress::Phase::Deployed | super::linux::progress::Phase::SystemBoot,
        ) => Stage::System,
        Some(_) => anyhow::bail!(
          "Ubuntu installation requires recovery; its disk and installer are preserved"
        ),
        None => Stage::Unattended,
      }
    } else {
      stage
    };
    check()?;
    if matches!(stage, Stage::Installer | Stage::Unattended) && machine.installer.is_none() {
      ensure!(
        machine.profile == "ubuntu",
        "Automatic Linux media requires the Ubuntu profile"
      );
      let media = crate::machines::linux::prepare(&self.manager.root, check.clone()).await?;
      machine.installer = Some(
        media
          .to_str()
          .ok_or_else(|| anyhow::anyhow!("Linux media path must be UTF-8"))?
          .into(),
      );
    }
    let manager = self.manager.clone();
    let client = self.client.clone();
    tokio::task::spawn_blocking(move || {
      let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
      let prepared = loop {
        match prepare::prepare(
          manager.clone(),
          machine.clone(),
          check.clone(),
          client.clone(),
          stage,
        ) {
          Ok(prepared) => break prepared,
          Err(error)
            if launch
              && stage == Stage::System
              && std::time::Instant::now() < deadline
              && error
                .chain()
                .filter_map(|error| error.downcast_ref::<std::io::Error>())
                .any(|error| error.kind() == std::io::ErrorKind::WouldBlock) =>
          {
            check()?;
            std::thread::sleep(std::time::Duration::from_millis(10));
          }
          Err(error) => return Err(error),
        }
      };
      if matches!(stage, Stage::Unattended) {
        prepared.unattended(&manager, actor)
      } else {
        Ok(prepared)
      }
    })
    .await?
  }

  pub fn installation(
    &self,
    id: &str,
    actor: Actor,
  ) -> anyhow::Result<Option<super::linux::progress::Phase>> {
    let original = self.manager.machine(id, actor)?;
    ensure!(
      original.guest == GuestOs::Linux
        && original.runtime == Some(model::MachineRuntime::Virtualization),
      "Installation status requires native Linux"
    );
    let directory = self.manager.root.join("vz").join(id);
    let phase = match std::fs::symlink_metadata(&directory) {
      Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
      Err(error) => return Err(error.into()),
      Ok(_) => super::linux::progress::read(&directory)?,
    };
    let current = self.manager.machine(id, actor)?;
    ensure!(
      current.guest == original.guest
        && current.runtime == original.runtime
        && (actor != Actor::Agent || current.agent_generation == original.agent_generation),
      "VM installation policy changed"
    );
    Ok(phase)
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
    let runtime = machine.runtime;
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
        machine.guest == guest && machine.runtime == runtime,
        "VM platform or runtime changed during the operation"
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
