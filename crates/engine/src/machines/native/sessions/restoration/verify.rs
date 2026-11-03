use super::Job;
use anyhow::Context;
use machine::{
  runtime::hardware,
  snapshot::{checkpoint, manifest::Binding, package::Package},
};

impl Job {
  pub(super) async fn verify_components(&self) -> anyhow::Result<()> {
    self.verify(false).await
  }

  pub(super) async fn verify_checkpoint(&self) -> anyhow::Result<()> {
    self.verify(true).await
  }

  async fn verify(&self, complete: bool) -> anyhow::Result<()> {
    let hardware = self
      .client
      .hardware
      .as_ref()
      .context("Missing restore hardware")?
      .clone();
    let manager = self.manager.clone();
    let id = self.id.clone();
    let snapshot = self.source.clone();
    let prior = complete.then(|| self.rollback.clone());
    let configuration = self.client.configuration;
    let check = self.check.clone();
    // the job retains the operation/session owners while bounded blocking reads finish.
    tokio::task::spawn_blocking(move || {
      check()?;
      let parent = hardware.namespace(&manager, &id)?;
      for snapshot in std::iter::once(snapshot).chain(prior) {
        let source = Package::open(
          &parent,
          Binding {
            machine: model::native::machine_identity(&id).context("Invalid restore VM identity")?,
            snapshot: model::native::machine_identity(&snapshot)
              .context("Invalid restore identity")?,
            configuration,
          },
          hardware.layout,
          || check(),
        )?;
        if complete {
          let (_, devices, _) =
            hardware::checkpoint(hardware.boot.clone(), &source, configuration, || check())?;
          drop(checkpoint::load(&source, &devices, || check())?);
        } else {
          source.verify_components(|| check())?;
        }
      }
      check()
    })
    .await
    .context("Restore source validation task failed")?
  }
}
