use super::{fs, Journal, Record};
use crate::machines::{
  native::{
    config,
    snapshot::{Commit, Policy},
  },
  Machines,
};
use machine::snapshot::manifest::Layout;
use std::{
  fs::OpenOptions,
  os::unix::fs::{MetadataExt, OpenOptionsExt},
  sync::Mutex,
};

pub struct Owner {
  journal: Journal,
  root: std::path::PathBuf,
  policy: Policy,
  _runtime: store::lock::Lease,
  operation: Mutex<Option<store::lock::Lease>>,
}

impl Owner {
  pub(crate) fn policy_generation(&self) -> anyhow::Result<u64> {
    self.check()?;
    if let Some(generation) = self.policy.generation() {
      return Ok(generation);
    }
    let manager = Machines {
      root: self.root.clone(),
    };
    let id = uuid::Uuid::from_bytes(self.policy.identity()).to_string();
    Ok(
      manager
        .machine(&id, crate::machines::Actor::Person)?
        .agent_generation,
    )
  }

  pub(crate) fn directory(&self) -> anyhow::Result<std::fs::File> {
    self.check_namespace()?;
    Ok(self.journal.root.try_clone()?)
  }

  pub(crate) fn continuing(
    manager: Machines,
    id: &str,
    policy: Policy,
    namespace: (u64, u64),
  ) -> anyhow::Result<Self> {
    anyhow::ensure!(
      Some(policy.identity()) == model::native::machine_identity(id),
      "Foreign continuation policy"
    );
    let operation = manager.lock(id)?;
    Self::open_owned(manager, id, policy, operation, Some(namespace))
  }

  pub(crate) fn namespace_stamp(&self) -> anyhow::Result<(u64, u64)> {
    self.check_namespace()?;
    let metadata = self.journal.root.metadata()?;
    Ok((metadata.dev(), metadata.ino()))
  }

  pub fn agent(manager: Machines, id: &str, generation: u64) -> anyhow::Result<Self> {
    let policy = Policy::agent(manager.clone(), id, generation)?;
    Self::open(manager, id, policy)
  }

  pub fn person(manager: Machines, id: &str) -> anyhow::Result<Self> {
    let policy = Policy::person(manager.clone(), id)?;
    Self::open(manager, id, policy)
  }

  fn open(manager: Machines, id: &str, policy: Policy) -> anyhow::Result<Self> {
    let operation = manager.lock(id)?;
    Self::open_owned(manager, id, policy, operation, None)
  }

  pub(crate) fn authorized(
    manager: Machines,
    id: &str,
    policy: Policy,
    operation: store::lock::Lease,
  ) -> anyhow::Result<Self> {
    Self::open_owned(manager, id, policy, operation, None)
  }

  pub(crate) fn with_operation(
    manager: Machines,
    id: &str,
    policy: Policy,
    operation: store::lock::Lease,
    namespace: (u64, u64),
  ) -> anyhow::Result<Self> {
    Self::open_owned(manager, id, policy, operation, Some(namespace))
  }

  fn open_owned(
    manager: Machines,
    id: &str,
    policy: Policy,
    operation: store::lock::Lease,
    namespace: Option<(u64, u64)>,
  ) -> anyhow::Result<Self> {
    anyhow::ensure!(
      Some(policy.identity()) == model::native::machine_identity(id),
      "Foreign restore operation policy"
    );
    let runtime = manager.guard(id, ".runtime")?;
    policy.check()?;
    let paths = config::paths(&manager.root, id)?;
    let root = OpenOptions::new()
      .read(true)
      .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
      .open(paths.root)?;
    fs::private(&root)?;
    let metadata = root.metadata()?;
    anyhow::ensure!(
      namespace.is_none_or(|stamp| stamp == (metadata.dev(), metadata.ino())),
      "Restore VM namespace changed before journal ownership"
    );
    let journal = Journal::open(&root, policy.identity())?;
    policy.check()?;
    Ok(Self {
      journal,
      root: manager.root.clone(),
      policy,
      _runtime: runtime,
      operation: Mutex::new(Some(operation)),
    })
  }

  pub(crate) fn check_namespace(&self) -> anyhow::Result<()> {
    self.namespace(
      &Machines {
        root: self.root.clone(),
      },
      &uuid::Uuid::from_bytes(self.policy.identity()).to_string(),
    )
  }

  pub(crate) fn boot(&self, boot: &model::native::Boot) -> anyhow::Result<()> {
    let manager = Machines {
      root: self.root.clone(),
    };
    let paths = config::paths(&self.root, &boot.machine_id)?;
    anyhow::ensure!(
      std::path::Path::new(&boot.store) == paths.variables,
      "Restored boot store differs from owner namespace"
    );
    self.namespace(&manager, &boot.machine_id)
  }

  pub(crate) fn activate(
    &self,
    expected: Record,
    mut check: impl FnMut() -> anyhow::Result<()>,
  ) -> anyhow::Result<Commit> {
    let manager = Machines {
      root: self.root.clone(),
    };
    let id = uuid::Uuid::from_bytes(expected.machine).to_string();
    self.journal.activate(expected, &self.policy, || {
      self.namespace(&manager, &id)?;
      check()
    })
  }

  pub(crate) fn namespace(&self, manager: &Machines, id: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
      manager.root == self.root
        && Some(self.policy.identity()) == model::native::machine_identity(id),
      "Restore owner belongs to another registry"
    );
    self.check()?;
    let path = config::paths(&manager.root, id)?.root;
    let root = OpenOptions::new()
      .read(true)
      .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
      .open(path)?;
    fs::private(&root)?;
    let original = self.journal.root.metadata()?;
    let current = root.metadata()?;
    anyhow::ensure!(
      original.dev() == current.dev() && original.ino() == current.ino(),
      "Restore VM namespace changed"
    );
    self.check()
  }

  pub(crate) fn release_operation(&self) -> anyhow::Result<()> {
    self.check()?;
    let lease = self
      .operation
      .lock()
      .map_err(|_| anyhow::anyhow!("Restore operation lock failed"))?
      .take()
      .ok_or_else(|| anyhow::anyhow!("Restore operation already transferred"))?;
    drop(lease);
    Ok(())
  }

  pub fn check(&self) -> anyhow::Result<()> {
    self.policy.check()
  }

  pub fn commit(&self) -> anyhow::Result<Commit> {
    self.policy.commit()
  }

  /// Authorization only; caller must retain this owner across every worker/core callback.
  pub fn policy(&self) -> Policy {
    self.policy.clone()
  }

  pub fn selected(&self) -> anyhow::Result<Option<Record>> {
    self.check()?;
    let selected = super::read(&self.journal.root, self.policy.identity())?;
    self.check()?;
    Ok(selected)
  }

  pub fn select(
    &mut self,
    record: Record,
    layout: Layout,
    check: impl FnMut() -> anyhow::Result<()>,
  ) -> anyhow::Result<()> {
    self.journal.select(record, layout, &self.policy, check)
  }

  pub fn attempted(
    &mut self,
    expected: Record,
    check: impl FnMut() -> anyhow::Result<()>,
  ) -> anyhow::Result<Record> {
    self.journal.attempted(expected, &self.policy, check)
  }

  /// Trusted recovery caller retains stopped-runtime ownership and admits both checkpoints.
  pub fn succeed(
    &mut self,
    expected: Record,
    next: Record,
    layout: Layout,
    mut check: impl FnMut() -> anyhow::Result<()>,
  ) -> anyhow::Result<()> {
    self.check_namespace()?;
    let id = uuid::Uuid::from_bytes(self.policy.identity()).to_string();
    let path = config::paths(&self.root, &id)?.root;
    let original = self.journal.root.metadata()?;
    self
      .journal
      .succeed(expected, next, layout, &self.policy, || {
        let root = OpenOptions::new()
          .read(true)
          .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
          .open(&path)?;
        fs::private(&root)?;
        let current = root.metadata()?;
        anyhow::ensure!(
          (original.dev(), original.ino()) == (current.dev(), current.ino()),
          "Restore successor namespace changed"
        );
        check()
      })
  }
}
