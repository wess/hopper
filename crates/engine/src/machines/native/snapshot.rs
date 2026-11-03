//! original saved policy for native snapshot work; runtime/operation ownership is separate.

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod hardware;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub(crate) use hardware::Hardware;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod catalog;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub use catalog::Entry;

use crate::machines::{Actor, Machines};
use anyhow::ensure;
use std::{
  io::{BufReader, Read},
  sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
  },
};

#[derive(Clone)]
pub struct Policy(Arc<Original>);

struct Original {
  manager: Machines,
  id: String,
  actor: Actor,
  generation: Option<u64>,
  failed: AtomicBool,
}

pub struct Commit {
  _lease: store::lock::Lease,
  _policy: Policy,
}

impl Policy {
  #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
  pub(crate) fn generation(&self) -> Option<u64> {
    self.0.generation
  }

  #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
  pub(crate) fn identity(&self) -> [u8; 16] {
    model::native::machine_identity(&self.0.id).expect("validated original policy identity")
  }

  /// `generation` belongs to the original request, before queueing or blocking work.
  pub fn agent(manager: Machines, id: &str, generation: u64) -> anyhow::Result<Self> {
    Self::new(manager, id, Actor::Agent, Some(generation))
  }

  /// Trusted local UI only; agent requests must use their original generation.
  pub fn person(manager: Machines, id: &str) -> anyhow::Result<Self> {
    Self::new(manager, id, Actor::Person, None)
  }

  fn new(
    manager: Machines,
    id: &str,
    actor: Actor,
    generation: Option<u64>,
  ) -> anyhow::Result<Self> {
    ensure!(
      manager.root.is_absolute(),
      "Snapshot VM root must be absolute"
    );
    model::native::machine_identity(id)
      .ok_or_else(|| anyhow::anyhow!("Invalid native VM identity"))?;
    let policy = Self(Arc::new(Original {
      manager,
      id: id.into(),
      actor,
      generation,
      failed: AtomicBool::new(false),
    }));
    policy.check()?;
    Ok(policy)
  }

  /// An observed failure permanently invalidates this request, including all cloned checks.
  pub fn check(&self) -> anyhow::Result<()> {
    let original = &self.0;
    let result = (|| {
      ensure!(
        !original.failed.load(Ordering::Acquire),
        "Snapshot policy is invalidated"
      );
      let file = super::assets::regular(&original.manager.record(&original.id)?, 1024 * 1024)?;
      let current: model::Machine =
        serde_json::from_reader(BufReader::new(file.take(1024 * 1024 + 1)))?;
      crate::machines::records::validate(&current)?;
      ensure!(
        current.id == original.id,
        "Snapshot record belongs to another VM"
      );
      ensure!(
        original.actor != Actor::Agent || current.agent_access,
        "Agent access is off for this VM. Enable it in Hopper first."
      );
      ensure!(
        current.guest == model::GuestOs::Windows
          && current.runtime == Some(model::MachineRuntime::Hypervisor),
        "Snapshot VM runtime has changed"
      );
      ensure!(
        original
          .generation
          .is_none_or(|generation| generation == current.agent_generation),
        "Snapshot agent policy has changed"
      );
      ensure!(
        !original.failed.load(Ordering::Acquire),
        "Snapshot policy is invalidated"
      );
      Ok(())
    })();
    if result.is_err() {
      original.failed.store(true, Ordering::Release);
    }
    result
  }

  /// Trusted worker handoff only. Caller retains original VM operation/runtime ownership.
  /// The parent namespace and expected configuration come from the host, never agent paths.
  #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
  pub fn publish(
    &self,
    parent: &std::fs::File,
    receipt: machine::snapshot::package::Receipt,
    binding: machine::snapshot::manifest::Binding,
    layout: machine::snapshot::manifest::Layout,
  ) -> anyhow::Result<machine::snapshot::package::Package> {
    self.publish_checked(parent, receipt, binding, layout, None, || Ok(()))
  }

  #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
  pub(crate) fn publish_checked(
    &self,
    parent: &std::fs::File,
    receipt: machine::snapshot::package::Receipt,
    binding: machine::snapshot::manifest::Binding,
    layout: machine::snapshot::manifest::Layout,
    description: Option<&machine::snapshot::package::Description>,
    mut check: impl FnMut() -> anyhow::Result<()>,
  ) -> anyhow::Result<machine::snapshot::package::Package> {
    ensure!(
      Some(binding.machine) == model::native::machine_identity(&self.0.id),
      "Snapshot publication belongs to another VM"
    );
    let mut authorize = || {
      self.check()?;
      check()?;
      self.check()
    };
    let mut staged =
      machine::snapshot::package::Sealed::open(parent, receipt, binding, layout, &mut authorize)?;
    if let Some(description) = description {
      staged.describe(description, &mut authorize)?;
    }
    staged.publish(authorize, || self.commit())
  }

  /// Use as the writer's commit factory, never as a long-lived serialization lease.
  /// Access changes take this same lock. Recheck after acquisition to close the check/lock race.
  pub fn commit(&self) -> anyhow::Result<Commit> {
    self.check()?;
    let lease = self.0.manager.guard(&self.0.id, ".access")?;
    self.check()?;
    Ok(Commit {
      _lease: lease,
      _policy: self.clone(),
    })
  }
}
