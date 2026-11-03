use super::super::{restore::Disposition, Sessions};
use crate::machines::native::{
  restore::{self, Owner, Phase, Prepared},
  snapshot::Policy,
};
use anyhow::{ensure, Context};
use std::{
  path::Path,
  sync::Arc,
  time::{Duration, Instant},
};

impl Sessions {
  /// Trusted prepared handoff with one automatic prior-checkpoint recovery attempt.
  /// Caller must capture/admit the prior state before preparation; no host paths are agent input.
  pub async fn start_recoverable(&self, helper: &Path, prepared: Prepared) -> anyhow::Result<()> {
    self
      .start_recoverable_in_state(helper, prepared, false)
      .await
  }

  pub async fn start_recoverable_paused(
    &self,
    helper: &Path,
    prepared: Prepared,
  ) -> anyhow::Result<()> {
    self
      .start_recoverable_in_state(helper, prepared, true)
      .await
  }

  pub(super) async fn activate_prepared(
    &self,
    helper: &Path,
    prepared: Prepared,
    paused: bool,
  ) -> anyhow::Result<()> {
    self
      .activate_in_state(
        helper,
        prepared,
        if paused {
          Disposition::Paused
        } else {
          Disposition::Running
        },
      )
      .await
  }

  pub(super) async fn activate_in_state(
    &self,
    helper: &Path,
    prepared: Prepared,
    state: Disposition,
  ) -> anyhow::Result<()> {
    let id = prepared.boot.machine_id.clone();
    match state {
      Disposition::Paused => {
        self
          .start_restored_paused(&id, helper, prepared.boot, prepared.restore, prepared.owner)
          .await
      }
      Disposition::Stopped => {
        self
          .start_restored_stopped(&id, helper, prepared.boot, prepared.restore, prepared.owner)
          .await
      }
      Disposition::Running => {
        self
          .start_restored(&id, helper, prepared.boot, prepared.restore, prepared.owner)
          .await
      }
    }
  }

  pub(super) async fn start_recoverable_in_state(
    &self,
    helper: &Path,
    prepared: Prepared,
    paused: bool,
  ) -> anyhow::Result<()> {
    let state = if paused {
      Disposition::Paused
    } else {
      Disposition::Running
    };
    self.recoverable(helper, prepared, state, state).await
  }

  pub async fn start_recoverable_stopped(
    &self,
    helper: &Path,
    prepared: Prepared,
  ) -> anyhow::Result<()> {
    self
      .recoverable(helper, prepared, Disposition::Running, Disposition::Stopped)
      .await
  }

  async fn recoverable(
    &self,
    helper: &Path,
    prepared: Prepared,
    requested: Disposition,
    prior: Disposition,
  ) -> anyhow::Result<()> {
    let id = prepared.boot.machine_id.clone();
    prepared.owner.boot(&prepared.boot)?;
    let selected = prepared
      .owner
      .selected()?
      .context("Missing recoverable selection")?;
    ensure!(
      selected.phase == Phase::Attempted
        && Some(selected.snapshot)
          == model::native::machine_identity(&prepared.restore.snapshot_id)
        && Some(selected.generation)
          == model::native::machine_identity(&prepared.restore.generation_id)
        && prepared
          .owner
          .policy()
          .generation()
          .is_none_or(|generation| generation == prepared.restore.policy_generation),
      "Recoverable handoff requires attempted intent"
    );
    let namespace = prepared.owner.namespace_stamp()?;
    let policy = prepared.owner.policy();
    let slot = self.slot(&id).await?;
    let epoch = *slot
      .generation
      .lock()
      .map_err(|_| anyhow::anyhow!("VM generation lock failed"))?;
    let boot = prepared.boot.clone();
    let result = self.activate_in_state(helper, prepared, requested).await;
    let Err(failure) = result else {
      return Ok(());
    };
    // Original authorization is retained; revocation must never acquire a fresh grant.
    let ownership = async {
      policy.check()?;
      let owner = acquire(self, &id, &policy, namespace).await?;
      ensure!(
        *slot
          .generation
          .lock()
          .map_err(|_| anyhow::anyhow!("VM generation lock failed"))?
          == epoch,
        "VM session changed before automatic recovery"
      );
      let current = owner
        .selected()?
        .context("Failed restore selection disappeared")?;
      ensure!(
        current == selected
          || current
            == restore::Record {
              phase: Phase::Active,
              ..selected
            },
        "Failed restore selection changed before recovery"
      );
      Ok::<_, anyhow::Error>((owner, current))
    }
    .await;
    let (owner, current) = match ownership {
      Ok(value) => value,
      Err(error) => return Err(failure.context(format!(
        "Restore failed and prior-state recovery did not complete; retained checkpoints require recovery: {error:#}"
      ))),
    };
    let original = policy.clone();
    let generation = slot.generation.clone();
    let deadline = Instant::now() + Duration::from_secs(300);
    let recovered = restore::recover_preparation(owner, failure, restore::Recovery {
      boot,
      target: uuid::Uuid::from_bytes(current.snapshot).to_string(),
      prior: uuid::Uuid::from_bytes(current.rollback).to_string(),
      predecessor: Some(current),
      configuration: current.configuration,
      check: Arc::new(move || {
        original.check()?;
        ensure!(Instant::now() < deadline, "Admission recovery deadline expired");
        ensure!(*generation.lock().map_err(|_| anyhow::anyhow!("VM generation lock failed"))? == epoch, "VM session changed during automatic recovery");
        Ok(())
      }),
    }).await.context("Restore failed and prior-state recovery did not complete; retained checkpoints require recovery")?;
    let restore::Recovered { prepared, failure } = recovered;
    // A failed recovery is preserved; never recursively restore alternating checkpoints.
    let recovery = self.activate_in_state(helper, prepared, prior).await;
    match recovery {
      Ok(()) => Err(failure.context("Restore failed; the saved prior checkpoint was recovered")),
      Err(error) => Err(failure.context(format!(
        "Restore failed and prior-state recovery did not complete; retained checkpoints require recovery: {error:#}"
      ))),
    }
  }
}

async fn acquire(
  sessions: &Sessions,
  id: &str,
  policy: &Policy,
  namespace: (u64, u64),
) -> anyhow::Result<Owner> {
  let deadline = Instant::now() + Duration::from_secs(2);
  loop {
    policy.check()?;
    match Owner::continuing(
      sessions.inner.manager.clone(),
      id,
      policy.clone(),
      namespace,
    ) {
      Ok(owner) => return Ok(owner),
      Err(error)
        if Instant::now() < deadline
          && error.chain().any(|cause| {
            cause
              .downcast_ref::<std::io::Error>()
              .is_some_and(|error| error.kind() == std::io::ErrorKind::WouldBlock)
          }) =>
      {
        tokio::time::sleep(Duration::from_millis(10)).await
      }
      Err(error) => {
        return Err(error.context("Failed worker ownership has not released for recovery"))
      }
    }
  }
}
