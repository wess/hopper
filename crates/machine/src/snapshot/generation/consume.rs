use super::{attempt, Generation};
use crate::{
  hypervisor::state::group::Checkpoint,
  runtime::{recovery::Disks, variables::Variables},
};

impl Generation {
  /// Permanently record an attempt before returning any executable resources. Original policy
  /// and operation/runtime ownership also scope this handoff. Failures never clear the marker.
  /// The host must retain a recoverable prior generation before calling this method.
  pub fn consume<G>(
    mut self,
    mut authorize: impl FnMut() -> anyhow::Result<()> + 'static,
    acquire: impl FnOnce() -> anyhow::Result<G>,
  ) -> anyhow::Result<(Checkpoint, Variables, Disks)> {
    self.check(&mut authorize)?;
    let _guard = acquire()?;
    authorize()?;
    self.namespace()?;
    let marker = attempt::write(&self.directory, &self.receipt)?;
    authorize()?;
    self.components()?;
    attempt::check(&self.directory, &marker)?;
    self.disks.authorize = Box::new(authorize);
    Ok((self.checkpoint, self.store, self.disks))
  }
}
