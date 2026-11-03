use super::{Disposition, Sessions, Start};
use crate::machines::native::restore::Owner;
use model::native::{Boot, Restore};
use std::path::Path;

impl Sessions {
  /// trusted prepared handoff only; no host paths or restore commands enter agent dispatch.
  pub async fn start_restored(
    &self,
    id: &str,
    helper: &Path,
    boot: Boot,
    restore: Restore,
    owned: Owner,
  ) -> anyhow::Result<()> {
    self
      .start_selected(
        id,
        helper,
        boot,
        restore,
        owned,
        Start::Restore(Disposition::Running),
      )
      .await
      .map(|_| ())
  }

  /// activates admitted checkpoint hardware without releasing its CPU hold.
  pub async fn start_restored_paused(
    &self,
    id: &str,
    helper: &Path,
    boot: Boot,
    restore: Restore,
    owned: Owner,
  ) -> anyhow::Result<()> {
    self
      .start_selected(
        id,
        helper,
        boot,
        restore,
        owned,
        Start::Restore(Disposition::Paused),
      )
      .await
      .map(|_| ())
  }

  /// activates and reaps held checkpoint hardware without publishing an executable session.
  pub async fn start_restored_stopped(
    &self,
    id: &str,
    helper: &Path,
    boot: Boot,
    restore: Restore,
    owned: Owner,
  ) -> anyhow::Result<()> {
    self
      .start_selected(
        id,
        helper,
        boot,
        restore,
        owned,
        Start::Restore(Disposition::Stopped),
      )
      .await
      .map(|_| ())
  }

  pub async fn start_generation(
    &self,
    id: &str,
    helper: &Path,
    boot: Boot,
    restore: Restore,
    owned: Owner,
  ) -> anyhow::Result<()> {
    self
      .start_selected(id, helper, boot, restore, owned, Start::Generation)
      .await
      .map(|_| ())
  }
}
