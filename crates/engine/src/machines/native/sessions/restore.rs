mod handoff;
mod stopped;
use super::{
  generation::{self, Preparation},
  owner, Sessions,
};
use crate::machines::{
  native::{self, restore::Owner, State},
  Actor,
};
use anyhow::{ensure, Context};
use model::native::{Boot, Command, Restore};
use std::{path::Path, sync::Arc};

#[derive(Clone, Copy)]
pub(super) enum Disposition {
  Running,
  Paused,
  Stopped,
}

enum Start {
  Restore(Disposition),
  Generation,
  Configured(native::assets::Assets),
}

impl Sessions {
  pub(super) async fn configured_generation(
    &self,
    id: &str,
    assets: &native::assets::Assets,
    stage: native::config::Stage,
  ) -> anyhow::Result<bool> {
    let manager = &self.inner.manager;
    let paths = native::config::paths(&manager.root, id)?;
    let Some(selected) = native::restore::selection(&paths.root, id)? else {
      return Ok(false);
    };
    ensure!(
      matches!(stage, native::config::Stage::System)
        && selected.phase == native::restore::Phase::Active,
      "Native restore requires recovery before startup"
    );
    // the unlocked probe only chooses a route; the owner rechecks under both VM leases.
    let owned = Owner::person(manager.clone(), id)?;
    ensure!(
      owned.selected()? == Some(selected),
      "Restore selection changed before startup"
    );
    let boot = native::config::prepare_generation(manager, id, assets, &owned)?;
    let restore = Restore {
      snapshot_id: uuid::Uuid::from_bytes(selected.snapshot).to_string(),
      generation_id: uuid::Uuid::from_bytes(selected.generation).to_string(),
      policy_generation: manager.machine(id, Actor::Person)?.agent_generation,
    };
    self
      .start_selected(
        id,
        &assets.worker,
        boot,
        restore,
        owned,
        Start::Configured(assets.clone()),
      )
      .await?;
    Ok(true)
  }

  async fn start_selected(
    &self,
    id: &str,
    helper: &Path,
    boot: Boot,
    restore: Restore,
    owned: Owner,
    start: Start,
  ) -> anyhow::Result<u64> {
    ensure!(
      boot.machine_id == id,
      "Restored boot belongs to a different VM"
    );
    owned.namespace(&self.inner.manager, id)?;
    let selected = owned.selected()?.context("No host restore selection")?;
    ensure!(
      Some(selected.machine) == model::native::machine_identity(id),
      "Restored owner belongs to a different VM"
    );
    let machine = self.inner.manager.machine(id, Actor::Person)?;
    ensure!(
      machine.guest == model::GuestOs::Windows
        && machine.runtime == Some(model::MachineRuntime::Hypervisor)
        && !self.inner.manager.root.join("lima").join(id).try_exists()?,
      "This VM requires migration before restored startup"
    );
    let slot = self.slot(id).await?;
    let mut session = slot.session.clone().lock_owned().await;
    if let Some(previous) = session.as_ref() {
      ensure!(
        matches!(
          native::state(&previous.client),
          State::Stopped(_) | State::Failed(_)
        ),
        "VM is already running"
      );
      owner::finished(previous).await?;
      session.take();
    }
    let owned = Arc::new(owned);
    owned.namespace(&self.inner.manager, id)?;
    let system = !matches!(start, Start::Restore(_));
    let paused = matches!(
      start,
      Start::Restore(Disposition::Paused | Disposition::Stopped)
    );
    let stopped = matches!(start, Start::Restore(Disposition::Stopped));
    let preparation = match start {
      Start::Configured(assets) => Preparation::Configured(assets),
      _ => Preparation::Prepared {
        helper: helper.to_owned(),
        boot: boot.clone(),
      },
    };
    let namespace = owned.namespace_stamp()?;
    let registry = Arc::downgrade(&self.inner);
    let helper = helper.to_owned();
    let manager = self.inner.manager.clone();
    let identity = id.to_owned();
    let (mut completion, result) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
      let startup = async {
        if system {
          native::launch_generation(&helper, boot, restore.clone(), owned.clone()).await
        } else {
          native::launch_restored(&helper, boot, restore.clone(), owned.clone()).await
        }
      };
      // dropping the helper future cancels its owned task without abandoning its leases.
      let client = tokio::select! {
        result = startup => match result {
          Ok(client) => client,
          Err(error) => { let _ = completion.send(Err(error)); return; }
        },
        _ = completion.closed() => return,
      };
      let resumed = tokio::select! {
        result = async {
          if system { native::resume_generation(&client, restore, owned.clone()).await }
          else if paused { native::pause_restored(&client, restore, owned.clone()).await }
          else { native::resume_restored(&client, restore, owned.clone()).await }
        } => result,
        _ = completion.closed() => return,
      };
      let activated = || -> anyhow::Result<native::restore::Record> {
        owned.namespace(&manager, &identity)?;
        ensure!(!completion.is_closed(), "Restored startup caller cancelled");
        let active = owned
          .selected()?
          .context("Restore activation disappeared")?;
        ensure!(
          active
            == native::restore::Record {
              phase: native::restore::Phase::Active,
              ..selected
            },
          "Restore activation differs from original selection"
        );
        Ok(active)
      };
      if stopped && resumed.is_ok() {
        let parked = stopped::finish(&client, &owned).await.and_then(|()| {
          let _commit = owned.commit()?;
          activated()?;
          owner::retire(&slot, native::state(&client));
          Ok(
            *slot
              .generation
              .lock()
              .map_err(|_| anyhow::anyhow!("VM generation lock failed"))?,
          )
        });
        let _ = completion.send(parked);
        return;
      }
      let admitted = resumed.and_then(|()| {
        let _commit = owned.commit()?;
        let active = activated()?;
        owned.release_operation()?;
        let created = owner::restored(client.clone(), Arc::downgrade(&owned));
        let epoch = owner::publish(&slot, &client);
        *session = Some(created);
        generation::monitor(
          registry,
          slot.clone(),
          generation::Context {
            manager: manager.clone(),
            id: identity.clone(),
            selected: active,
            policy: owned.policy(),
            namespace,
            preparation,
          },
          epoch,
        );
        Ok(epoch)
      });
      if admitted.is_err() {
        let _ = native::request(&client, Command::Stop {}).await;
        let _ = native::finished(&client).await;
      }
      // publication and completion happen together; later caller cancellation cannot undo admission.
      let _ = completion.send(admitted);
    });
    result
      .await
      .context("Restored session owner ended without completion")?
  }
}
