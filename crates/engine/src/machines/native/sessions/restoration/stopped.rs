use super::{
  super::{owner, restore::Disposition, Sessions},
  Cancel, Restored,
};
use crate::machines::{
  native::{
    self,
    restore::{Owner, Phase},
    Check, State,
  },
  Actor,
};
use anyhow::{ensure, Context};
use machine::snapshot::{
  manifest::Binding,
  package::{Description, Package},
};
use model::native::{Boot, Command, Restore, Result as Reply};
use std::{
  path::Path,
  sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
  },
  time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

impl Sessions {
  pub(super) async fn restore_configured_stopped(
    &self,
    id: &str,
    snapshot: &str,
    policy: native::snapshot::Policy,
    operation: store::lock::Lease,
  ) -> anyhow::Result<Restored> {
    let owned = Owner::authorized(self.inner.manager.clone(), id, policy, operation)?;
    let assets = native::assets::locate()?;
    let boot = native::config::prepare_generation(&self.inner.manager, id, &assets, &owned)?;
    self
      .restore_stopped(&assets.worker, boot, snapshot, owned)
      .await
  }

  /// Trusted stopped current-generation restore. Boot/helper paths are never agent input.
  pub async fn restore_stopped(
    &self,
    helper: &Path,
    boot: Boot,
    snapshot: &str,
    owned: Owner,
  ) -> anyhow::Result<Restored> {
    let id = boot.machine_id.clone();
    owned.boot(&boot)?;
    owned.namespace(&self.inner.manager, &id)?;
    let machine = self.inner.manager.machine(&id, Actor::Person)?;
    ensure!(
      machine.guest == model::GuestOs::Windows
        && machine.runtime == Some(model::MachineRuntime::Hypervisor)
        && !self
          .inner
          .manager
          .root
          .join("lima")
          .join(&id)
          .try_exists()?,
      "Stopped restore requires native Windows"
    );
    if let Some(installed) = native::installation::read(&self.inner.manager, &id, Actor::Person)? {
      ensure!(
        matches!(installed, model::native::Installation::SystemStarted {}),
        "Stopped restore requires an installed system outside setup/recovery"
      );
    }
    let selected = owned
      .selected()?
      .context("Stopped restore needs an active current generation")?;
    ensure!(
      selected.phase == Phase::Active,
      "Stopped restore selection requires recovery"
    );
    let hardware = native::snapshot::Hardware::capture(&boot, helper)?;
    let snapshots = hardware.namespace(&self.inner.manager, &id)?;
    owned.namespace(&self.inner.manager, &id)?;
    let snapshot_id =
      model::native::machine_identity(snapshot).context("Invalid stopped restore snapshot")?;
    let package = Package::inspect(&snapshots, selected.machine, snapshot_id, || owned.check())?;
    ensure!(
      package.binding().configuration == selected.configuration,
      "Stopped restore snapshot hardware differs"
    );
    let layout = package.layout();
    ensure!(
      layout == hardware.layout,
      "Stopped restore snapshot topology differs"
    );
    drop(package);
    let slot = self.slot(&id).await?;
    let mut session = slot.session.clone().lock_owned().await;
    if let Some(previous) = session.as_ref() {
      ensure!(
        matches!(
          native::state(&previous.client),
          State::Stopped(_) | State::Failed(_)
        ),
        "Stopped restore source is running"
      );
      owner::finished(previous).await?;
      session.take();
    }
    let cancelled = Cancel(Arc::new(AtomicBool::new(false)));
    let cancel = cancelled.0.clone();
    let source = snapshot.to_owned();
    let prior = model::new_uuid();
    let receipt_prior = prior.clone();
    let helper = helper.to_owned();
    let prepared_helper = helper.clone();
    let tag = owned.policy_generation()?;
    let current = Restore {
      snapshot_id: uuid::Uuid::from_bytes(selected.snapshot).to_string(),
      generation_id: uuid::Uuid::from_bytes(selected.generation).to_string(),
      policy_generation: tag,
    };
    let owned = Arc::new(owned);
    let authority = owned.clone();
    let context = boot.clone();
    let deadline = Instant::now() + Duration::from_secs(300);
    let checked_cancel = cancel.clone();
    let check: Check = Arc::new(move || {
      ensure!(
        !checked_cancel.load(Ordering::Acquire),
        "Stopped restore caller cancelled"
      );
      ensure!(
        Instant::now() < deadline,
        "Stopped prior capture deadline expired"
      );
      authority.boot(&context)?;
      ensure!(
        authority.selected()? == Some(selected),
        "Stopped restore predecessor changed"
      );
      Ok(())
    });
    check()?;
    let (completion, result) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
      // Session, operation and runtime ownership outlive caller cancellation and worker cleanup.
      let outcome = async {
        let client = tokio::select! {
          result = native::launch_generation(&helper, boot.clone(), current, owned.clone()) => result?,
          _ = async { while !cancel.load(Ordering::Acquire) { tokio::time::sleep(Duration::from_millis(50)).await; } }
            => anyhow::bail!("Stopped prior startup cancelled"),
        };
        let captured = async {
          check()?;
          ensure!(matches!(native::state(&client), State::Paused), "Stopped prior escaped its hold");
          let receipt = super::super::snapshot::control::capture(&client, &prior, tag, &check).await?;
          let policy = owned.policy();
          let checked = check.clone();
          let binding = Binding { machine: selected.machine,
            snapshot: model::native::machine_identity(&prior).unwrap(), configuration: selected.configuration };
          let name = Description::new("Before restore", SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())?;
          tokio::task::spawn_blocking(move || {
            drop(policy.publish_checked(&snapshots, receipt, binding, layout, Some(&name), || checked())?);
            checked()
          }).await.context("Stopped prior publication task failed")??;
          check()
        }.await;
        let cleanup = super::super::snapshot::control::cleanup(&client, &prior, tag).await;
        let stopped = native::request(&client, Command::Stop {}).await;
        if let Err(error) = native::finished(&client).await {
          std::mem::forget((owned, session));
          return Err(error.context("Stopped prior cleanup is unverified; ownership retained"));
        }
        captured?;
        cleanup?;
        ensure!(matches!(stopped?, Reply::Stopped { .. }), "Stopped prior did not acknowledge stop");
        check()?;
        owner::retire(&slot, native::state(&client));
        drop(client);
        drop(check);
        // The process monitor publishes reaping just before dropping its retained scope.
        let released = tokio::time::timeout(Duration::from_secs(2), async {
          while Arc::strong_count(&owned) != 1 { tokio::time::sleep(Duration::from_millis(1)).await; }
        }).await;
        if released.is_err() {
          std::mem::forget((owned, session));
          anyhow::bail!("Stopped prior scope cleanup is unverified; ownership retained");
        }
        let owned = Arc::try_unwrap(owned).map_err(|_| anyhow::anyhow!("Stopped prior owner remained shared"))?;
        let epoch = *slot.generation.lock().map_err(|_| anyhow::anyhow!("VM generation lock failed"))?;
        let generation = slot.generation.clone();
        let policy = owned.policy();
        let checked_cancel = cancel.clone();
        let deadline = Instant::now() + Duration::from_secs(300);
        let continuation: Check = Arc::new(move || {
          ensure!(!checked_cancel.load(Ordering::Acquire), "Stopped restore preparation caller cancelled");
          ensure!(Instant::now() < deadline, "Stopped restore preparation deadline expired");
          policy.check()?;
          ensure!(*generation.lock().map_err(|_| anyhow::anyhow!("VM generation lock failed"))? == epoch,
            "VM generation changed during stopped restore preparation");
          Ok(())
        });
        let preparation = super::preparation::run(owned, native::restore::Recovery {
          boot,
          target: source,
          prior,
          predecessor: Some(selected),
          configuration: selected.configuration,
          check: continuation,
        });
        let prepared = tokio::select! {
          result = preparation => result?,
          _ = async { while !cancel.load(Ordering::Acquire) { tokio::time::sleep(Duration::from_millis(50)).await; } }
            => anyhow::bail!("Stopped restore preparation cancelled"),
        };
        drop(session);
        Ok::<_, anyhow::Error>(prepared)
      }.await;
      let _ = completion.send(outcome);
    });
    let (prepared, failure) = result
      .await
      .context("Stopped restore owner ended without completion")??;
    let receipt = Restored {
      snapshot_id: prepared.restore.snapshot_id.clone(),
      generation_id: prepared.restore.generation_id.clone(),
      rollback_id: receipt_prior,
    };
    if let Some(failure) = failure {
      let recovered = self
        .activate_in_state(&prepared_helper, prepared, Disposition::Stopped)
        .await;
      return match recovered {
        Ok(()) => Err(failure.context("Stopped restore preparation failed; the saved prior checkpoint was recovered without starting the guest")),
        Err(error) => Err(failure.context(format!("Stopped restore preparation failed and prior recovery did not complete; retained checkpoints require recovery: {error:#}"))),
      };
    }
    self
      .start_recoverable_stopped(&prepared_helper, prepared)
      .await?;
    drop(cancelled);
    Ok(receipt)
  }
}
