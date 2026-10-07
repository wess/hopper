use super::{prepare, Client, Owner, Prepared, Service, Stage, State, Status};
use crate::machines::{
  linux::progress::{self, Phase},
  Actor, Machines,
};
use anyhow::{ensure, Context};
use machine::vz::queue::Check;
use model::{GuestOs, Machine, MachineRuntime};
use std::{
  sync::Arc,
  time::{Duration, Instant},
};

#[derive(Clone)]
pub struct Installation {
  manager: Machines,
  machine: Machine,
  actor: Actor,
  attempt: String,
  client: Client,
  generation: u64,
  intent: Option<String>,
}

pub struct Permit {
  installation: Installation,
  check: Check,
}

impl Installation {
  pub(super) fn new(
    service: Service,
    machine: Machine,
    actor: Actor,
    attempt: String,
    generation: u64,
    intent: Option<String>,
  ) -> anyhow::Result<Self> {
    progress::Decoder::new(&attempt)?;
    ensure!(
      generation > 0
        && machine.guest == GuestOs::Linux
        && machine.runtime == Some(MachineRuntime::Virtualization)
        && machine.profile == "ubuntu",
      "Installation watch requires owned native Ubuntu"
    );
    let installation = Self {
      manager: service.manager,
      machine,
      actor,
      attempt,
      client: service.client,
      generation,
      intent,
    };
    installation.check()?;
    Ok(installation)
  }

  pub fn generation(&self) -> u64 {
    self.generation
  }

  pub fn id(&self) -> &str {
    &self.machine.id
  }
  pub fn name(&self) -> &str {
    &self.machine.name
  }

  fn check(&self) -> anyhow::Result<()> {
    authorized(&self.manager, &self.machine, self.actor)?;
    ensure!(
      super::intent::read(&self.manager, self.id())? == self.intent,
      "VM startup cancelled by an explicit stop request"
    );
    Ok(())
  }

  fn access(&self) -> Check {
    let original = self.clone();
    Arc::new(move || original.check())
  }

  fn phase(&self) -> anyhow::Result<Option<Phase>> {
    progress::read_attempt(&self.manager.root.join("vz").join(self.id()), &self.attempt)
  }

  fn same(&self, status: Status) -> anyhow::Result<()> {
    ensure!(
      status.generation == self.generation && status.installer,
      "VM installation runtime was replaced"
    );
    ensure!(
      !status.stop_requested,
      "Automatic startup cancelled by an explicit VM stop"
    );
    ensure!(
      status.state != State::Error,
      "Installer hardware failed; its disk is preserved"
    );
    Ok(())
  }

  pub async fn wait(self) -> anyhow::Result<Self> {
    let mut active = Duration::ZERO;
    let mut previous = Instant::now();
    let mut paused = false;
    let mut stopped = None;
    loop {
      self.check()?;
      let status = self
        .client
        .status_checked(self.id(), self.access())
        .await?
        .context("Installer runtime is no longer owned")?;
      self.same(status)?;
      let now = Instant::now();
      if !paused {
        active = active.saturating_add(now.duration_since(previous));
      }
      previous = now;
      paused = matches!(status.state, State::Paused | State::Pausing);
      let phase = self.phase()?;
      self.check()?;
      ensure!(
        phase != Some(Phase::Failed),
        "Ubuntu installation failed; its disk is preserved for recovery"
      );
      if status.state == State::Stopped && status.started && !status.busy {
        if phase == Some(Phase::Deployed) {
          return Ok(self);
        }
        let observed = *stopped.get_or_insert_with(Instant::now);
        ensure!(
          observed.elapsed() < Duration::from_secs(5),
          "Installer stopped without deployment completion; its disk is preserved for recovery"
        );
      }
      ensure!(
        active < Duration::from_secs(12 * 60 * 60),
        "Automatic installation watch timed out; its disk is preserved"
      );
      tokio::time::sleep(Duration::from_millis(250)).await;
    }
  }

  pub fn permit(self, owner: &Owner) -> anyhow::Result<Permit> {
    self.check()?;
    let lease = Arc::new(self.manager.lock(self.id())?);
    self.check()?;
    let status = owner.inspect(self.id())?;
    self.same(status)?;
    ensure!(
      status.started && status.state == State::Stopped && !status.busy,
      "Installer is not ready for system boot"
    );
    ensure!(
      self.phase()? == Some(Phase::Deployed),
      "Installation completion changed; its disk is preserved"
    );
    owner.can_replace(self.id())?;
    let original = self.clone();
    let check: Check = Arc::new(move || {
      let _lease = &lease;
      original.check()
    });
    Ok(Permit {
      installation: self,
      check,
    })
  }
}

impl Permit {
  pub fn retire(&self, owner: &mut Owner) -> anyhow::Result<()> {
    (self.check)()?;
    let status = owner.inspect(self.installation.id())?;
    self.installation.same(status)?;
    ensure!(
      self.installation.phase()? == Some(Phase::Deployed),
      "Installation completion changed; its disk is preserved"
    );
    owner.retire(self.installation.id())
  }

  pub async fn prepare(self) -> anyhow::Result<Prepared> {
    (self.check)()?;
    tokio::task::spawn_blocking(move || {
      let deadline = Instant::now() + Duration::from_millis(500);
      loop {
        (self.check)()?;
        ensure!(
          self.installation.phase()? == Some(Phase::Deployed),
          "Installation completion changed; its disk is preserved"
        );
        match prepare::prepare(
          self.installation.manager.clone(),
          self.installation.machine.clone(),
          self.check.clone(),
          self.installation.client.clone(),
          Stage::System,
        ) {
          Ok(prepared) => {
            return Ok(prepared.control(self.installation.actor, self.installation.intent.clone()))
          }
          Err(error)
            if Instant::now() < deadline
              && error
                .chain()
                .filter_map(|error| error.downcast_ref::<std::io::Error>())
                .any(|error| error.kind() == std::io::ErrorKind::WouldBlock) =>
          {
            std::thread::sleep(Duration::from_millis(10))
          }
          Err(error) => return Err(error),
        }
      }
    })
    .await?
  }
}

fn authorized(manager: &Machines, original: &Machine, actor: Actor) -> anyhow::Result<()> {
  let current = manager.machine(&original.id, actor)?;
  ensure!(
    current.guest == original.guest
      && current.runtime == original.runtime
      && current.profile == original.profile,
    "VM installation platform changed"
  );
  ensure!(
    actor != Actor::Agent || current.agent_generation == original.agent_generation,
    "VM installation agent policy changed"
  );
  ensure!(
    !manager.root.join("lima").join(&original.id).try_exists()?,
    "Previous VM requires migration; its disk is preserved"
  );
  Ok(())
}

pub(super) fn access(
  manager: Machines,
  machine: Machine,
  actor: Actor,
  intent: Option<String>,
) -> Check {
  let id = machine.id.clone();
  let original = manager.clone();
  super::intent::checked(
    manager,
    id,
    intent,
    Arc::new(move || authorized(&original, &machine, actor)),
  )
}
