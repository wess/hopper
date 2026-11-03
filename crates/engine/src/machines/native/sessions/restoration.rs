mod capture;
mod preparation;
mod prepare;
mod rollback;
mod stopped;
mod verify;
use super::{Session, Sessions, Slot};
use crate::machines::{
  native::{self, config, restore::Prepared, snapshot::Policy, Check, State},
  Actor, Machines,
};
use anyhow::{ensure, Context};
use machine::snapshot::{manifest::Binding, package::Package};
use std::{
  path::PathBuf,
  sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
  },
  time::{Duration, Instant},
};
use tokio::sync::OwnedMutexGuard;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Restored {
  pub snapshot_id: String,
  pub rollback_id: String,
  pub generation_id: String,
}

struct Cancel(Arc<AtomicBool>);
impl Drop for Cancel {
  fn drop(&mut self) {
    self.0.store(true, Ordering::Release);
  }
}

struct Job {
  manager: Machines,
  id: String,
  source: String,
  rollback: String,
  client: native::Client,
  policy: Policy,
  check: Check,
  stop_check: Check,
  cancel: Arc<AtomicBool>,
  uncertain: AtomicBool,
  tag: u64,
  slot: Arc<Slot>,
  session: OwnedMutexGuard<Option<Session>>,
  operation: store::lock::Lease,
  predecessor: Option<native::restore::Record>,
}

struct Ready {
  prepared: Prepared,
  helper: PathBuf,
  rollback: String,
  failure: Option<anyhow::Error>,
  paused: bool,
}

impl Sessions {
  pub async fn restore_snapshot(
    &self,
    id: &str,
    actor: Actor,
    snapshot: &str,
  ) -> anyhow::Result<Restored> {
    self.restore_using(id, actor, snapshot, None).await
  }

  pub async fn restore_snapshot_with_generation(
    &self,
    id: &str,
    snapshot: &str,
    generation: u64,
  ) -> anyhow::Result<Restored> {
    self
      .restore_using(id, Actor::Agent, snapshot, Some(generation))
      .await
  }

  async fn restore_using(
    &self,
    id: &str,
    actor: Actor,
    snapshot: &str,
    generation: Option<u64>,
  ) -> anyhow::Result<Restored> {
    let machine = self.inner.manager.machine(id, actor)?;
    let tag = generation.unwrap_or(machine.agent_generation);
    let policy = match actor {
      Actor::Agent => Policy::agent(self.inner.manager.clone(), id, tag)?,
      Actor::Person => Policy::person(self.inner.manager.clone(), id)?,
    };
    let snapshot_id =
      model::native::machine_identity(snapshot).context("Invalid restore snapshot identity")?;
    let slot = self.slot(id).await?;
    let session = slot.session.clone().lock_owned().await;
    let operation = self.inner.manager.lock(id)?;
    policy.check()?;
    if session.as_ref().is_none_or(|session| {
      matches!(
        native::state(&session.client),
        State::Stopped(_) | State::Failed(_)
      )
    }) {
      drop(session);
      return self
        .restore_configured_stopped(id, snapshot, policy, operation)
        .await;
    }
    let active = session
      .as_ref()
      .context("Restore requires an owned running or paused source")?;
    ensure!(
      matches!(
        native::state(&active.client),
        State::Running | State::Paused
      ),
      "Restore source is unavailable"
    );
    let client = active.client.clone();
    let hardware = client
      .hardware
      .as_ref()
      .context("Restore source hardware binding is unavailable")?;
    ensure!(
      hardware.boot.boot_media.is_none() && hardware.boot.installer.is_none(),
      "Restore requires detached installation media"
    );
    let parent = hardware.namespace(&self.inner.manager, id)?;
    let paths = config::paths(&self.inner.manager.root, id)?;
    let predecessor = native::restore::selection(&paths.root, id)?;
    ensure!(
      predecessor.is_none_or(|record| {
        record.phase == native::restore::Phase::Active
          && record.configuration == client.configuration
      }),
      "Restore source selection requires recovery or has different hardware"
    );
    if let Some(installed) = native::installation::read(&self.inner.manager, id, actor)? {
      ensure!(
        matches!(installed, model::native::Installation::SystemStarted {}),
        "Restore requires an installed system outside setup/recovery"
      );
    }
    drop(Package::open(
      &parent,
      Binding {
        machine: model::native::machine_identity(id).context("Invalid VM identity")?,
        snapshot: snapshot_id,
        configuration: client.configuration,
      },
      hardware.layout,
      || policy.check(),
    )?);
    let epoch = *slot
      .generation
      .lock()
      .map_err(|_| anyhow::anyhow!("VM generation lock failed"))?;
    let cancelled = Cancel(Arc::new(AtomicBool::new(false)));
    let cancel = cancelled.0.clone();
    let original = policy.clone();
    let current = client.clone();
    let generation = slot.generation.clone();
    let deadline = Instant::now() + Duration::from_secs(300);
    let selection_root = paths.root;
    let identity = id.to_owned();
    let stop_check: Check = Arc::new(move || {
      ensure!(!cancel.load(Ordering::Acquire), "Restore caller cancelled");
      ensure!(
        Instant::now() < deadline,
        "Restore capture deadline expired"
      );
      original.check()?;
      ensure!(
        native::restore::selection(&selection_root, &identity)? == predecessor,
        "Restore source selection changed"
      );
      ensure!(
        *generation
          .lock()
          .map_err(|_| anyhow::anyhow!("VM generation lock failed"))?
          == epoch,
        "Restore source generation changed"
      );
      Ok(())
    });
    let check = stop_check.clone();
    let check: Check = Arc::new(move || {
      check()?;
      ensure!(
        matches!(native::state(&current), State::Running | State::Paused),
        "Restore source runtime is unavailable"
      );
      Ok(())
    });
    let job = Job {
      manager: self.inner.manager.clone(),
      id: id.into(),
      source: snapshot.into(),
      rollback: model::new_uuid(),
      client,
      policy,
      check,
      stop_check,
      cancel: cancelled.0.clone(),
      uncertain: AtomicBool::new(false),
      tag,
      slot,
      session,
      operation,
      predecessor,
    };
    let (completion, result) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
      let _ = completion.send(job.prepare().await);
    });
    let ready = result
      .await
      .context("Restore capture owner ended without completion")??;
    let receipt = Restored {
      snapshot_id: ready.prepared.restore.snapshot_id.clone(),
      rollback_id: ready.rollback,
      generation_id: ready.prepared.restore.generation_id.clone(),
    };
    if let Some(failure) = ready.failure {
      let prepared = ready.prepared;
      let recovery = self
        .activate_prepared(&ready.helper, prepared, ready.paused)
        .await;
      return match recovery {
        Ok(()) => Err(failure.context("Restore preparation failed; the saved prior checkpoint was recovered")),
        Err(error) => Err(failure.context(format!("Restore preparation failed and prior startup did not complete; retained checkpoints require recovery: {error:#}"))),
      };
    }
    self
      .start_recoverable_in_state(&ready.helper, ready.prepared, ready.paused)
      .await?;
    drop(cancelled);
    Ok(receipt)
  }
}
