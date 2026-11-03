pub(super) mod control;
use super::{
  super::{snapshot::Policy, State},
  Sessions,
};
use crate::machines::Actor;
use anyhow::{ensure, Context};
use machine::snapshot::manifest::Binding;
use std::{
  sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
  },
  time::{Duration, Instant},
};

struct Cancel(Arc<AtomicBool>);
impl Drop for Cancel {
  fn drop(&mut self) {
    self.0.store(true, Ordering::Release);
  }
}

impl Sessions {
  /// Native capture only; snapshot indexing and restore are separate operations.
  pub async fn snapshot(&self, id: &str, actor: Actor, snapshot: &str) -> anyhow::Result<()> {
    self.snapshot_using(id, actor, snapshot, None, None).await
  }

  /// Agent connections supply their original request generation, before upstream queueing.
  pub async fn snapshot_with_generation(
    &self,
    id: &str,
    snapshot: &str,
    generation: u64,
  ) -> anyhow::Result<()> {
    self
      .snapshot_using(id, Actor::Agent, snapshot, Some(generation), None)
      .await
  }

  pub async fn snapshot_named(
    &self,
    id: &str,
    actor: Actor,
    snapshot: &str,
    name: &str,
  ) -> anyhow::Result<()> {
    self
      .snapshot_using(id, actor, snapshot, None, Some(name))
      .await
  }

  pub async fn snapshot_named_with_generation(
    &self,
    id: &str,
    snapshot: &str,
    name: &str,
    generation: u64,
  ) -> anyhow::Result<()> {
    self
      .snapshot_using(id, Actor::Agent, snapshot, Some(generation), Some(name))
      .await
  }

  async fn snapshot_using(
    &self,
    id: &str,
    actor: Actor,
    snapshot: &str,
    generation: Option<u64>,
    name: Option<&str>,
  ) -> anyhow::Result<()> {
    let machine = self.inner.manager.machine(id, actor)?;
    let tag = generation.unwrap_or(machine.agent_generation);
    let policy = match actor {
      Actor::Agent => Policy::agent(self.inner.manager.clone(), id, tag)?,
      Actor::Person => Policy::person(self.inner.manager.clone(), id)?,
    };
    let description = name
      .map(|name| {
        let created = std::time::SystemTime::now()
          .duration_since(std::time::UNIX_EPOCH)?
          .as_secs();
        machine::snapshot::package::Description::new(name, created)
      })
      .transpose()?;
    let snapshot_id =
      model::native::machine_identity(snapshot).context("Invalid snapshot identity")?;
    let slot = self.slot(id).await?;
    let session = slot.session.clone().lock_owned().await;
    let operation = self.inner.manager.lock(id)?;
    policy.check()?;
    let active = session.as_ref().context("VM is not running")?;
    if let Some(installed) = super::super::installation::read(&self.inner.manager, id, actor)? {
      ensure!(
        matches!(installed, model::native::Installation::SystemStarted {}),
        "Snapshot capture requires an installed system outside setup/recovery"
      );
    }
    let client = active.client.clone();
    let hardware = client
      .hardware
      .as_ref()
      .context("Snapshot startup namespace binding is unavailable")?;
    let parent = hardware.namespace(&self.inner.manager, id)?;
    let layout = hardware.layout;
    let binding = Binding {
      machine: model::native::machine_identity(id).context("Invalid VM identity")?,
      snapshot: snapshot_id,
      configuration: client.configuration,
    };
    let epoch = *slot
      .generation
      .lock()
      .map_err(|_| anyhow::anyhow!("VM generation lock failed"))?;
    let cancelled = Cancel(Arc::new(AtomicBool::new(false)));
    let cancel = cancelled.0.clone();
    let generation = slot.generation.clone();
    let current = client.clone();
    let deadline = Instant::now() + Duration::from_secs(300);
    let original = policy.clone();
    let check: super::super::Check = Arc::new(move || {
      ensure!(!cancel.load(Ordering::Acquire), "Snapshot caller cancelled");
      ensure!(
        Instant::now() < deadline,
        "Snapshot operation deadline expired"
      );
      original.check()?;
      ensure!(
        *generation
          .lock()
          .map_err(|_| anyhow::anyhow!("VM generation lock failed"))?
          == epoch,
        "Snapshot runtime generation has changed"
      );
      ensure!(
        matches!(
          super::super::state(&current),
          State::Running | State::Paused
        ),
        "Snapshot source runtime is unavailable"
      );
      Ok(())
    });
    check()?;
    let snapshot = snapshot.to_owned();
    let (completion, result) = tokio::sync::oneshot::channel();
    // the caller cancels its flag; only this owner may release operation/session ownership.
    tokio::spawn(async move {
      let outcome = control::capture(&client, &snapshot, tag, &check).await;
      let outcome = match outcome {
        Ok(receipt) => {
          let checks = check.clone();
          tokio::task::spawn_blocking(move || {
            let package = policy.publish_checked(
              &parent,
              receipt,
              binding,
              layout,
              description.as_ref(),
              || checks(),
            )?;
            drop(package);
            checks()
          })
          .await
          .context("Snapshot commit task failed")
          .and_then(|result| result)
        }
        Err(error) => Err(error),
      };
      let cleanup = control::cleanup(&client, &snapshot, tag).await;
      if cleanup.is_err() {
        // uncertain cleanup must not release ownership while this worker could still write.
        let _ = super::super::request(&client, model::native::Command::Stop {}).await;
        if let Err(error) =
          super::owner::finished(session.as_ref().expect("retained snapshot session")).await
        {
          let _ = completion.send(Err(
            error.context("Snapshot cleanup is unverified; VM operation remains owned"),
          ));
          let _held = (operation, session);
          return std::future::pending::<()>().await;
        }
      }
      let outcome = outcome.and(cleanup).and_then(|()| check());
      drop(operation);
      drop(session);
      let _ = completion.send(outcome);
    });
    let outcome = result
      .await
      .context("Snapshot owner ended without completion")?;
    drop(cancelled);
    outcome
  }
}
