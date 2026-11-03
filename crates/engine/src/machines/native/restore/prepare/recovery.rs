use super::{stage, Cancel, Owner, Prepared, Record, Scope};
use crate::machines::native::{
  restore::{fs, Phase},
  Check,
};
use anyhow::{ensure, Context};
use machine::{
  runtime::hardware,
  snapshot::{
    checkpoint,
    manifest::Binding,
    package::{Description, Package},
  },
};
use model::native::Boot;
use std::{
  sync::{atomic::AtomicBool, Arc},
  time::{SystemTime, UNIX_EPOCH},
};

pub struct Recovery {
  pub boot: Boot,
  pub target: String,
  pub prior: String,
  pub predecessor: Option<Record>,
  pub configuration: [u8; 32],
  pub check: Check,
}

pub struct Recovered {
  pub prepared: Prepared,
  pub failure: anyhow::Error,
}

/// One trusted recovery preparation under the failed attempt's original owner and authorization.
/// The caller supplies the verified prior identity and independent original hardware binding.
pub async fn recover_preparation(
  mut owner: Owner,
  failure: anyhow::Error,
  recovery: Recovery,
) -> anyhow::Result<Recovered> {
  let cancelled = Arc::new(AtomicBool::new(false));
  let _cancel = Cancel(cancelled.clone());
  let (completion, result) = tokio::sync::oneshot::channel();
  tokio::task::spawn_blocking(move || {
    let prepared = (|| {
      owner.boot(&recovery.boot)?;
      (recovery.check)()?;
      let target = model::native::machine_identity(&recovery.target)
        .context("Invalid failed target identity")?;
      let prior = model::native::machine_identity(&recovery.prior)
        .context("Invalid recovery prior identity")?;
      ensure!(
        target != prior,
        "Recovery requires the distinct captured prior checkpoint"
      );
      let selected = owner.selected()?;
      ensure!(
        recovery
          .predecessor
          .is_none_or(|record| record.configuration == recovery.configuration),
        "Recovery predecessor configuration differs"
      );
      if selected != recovery.predecessor {
        let current = selected.context("Failed preparation selection disappeared")?;
        ensure!(
          matches!(current.phase, Phase::Prepared | Phase::Attempted)
            && current.snapshot == target
            && current.rollback == prior
            && current.configuration == recovery.configuration
            && current.previous == recovery.predecessor.map(|record| record.generation),
          "Failed preparation selection changed before recovery"
        );
      }
      let scope = Scope {
        root: owner.directory()?,
        path: std::path::Path::new(&recovery.boot.store)
          .parent()
          .context("Missing recovery namespace")?
          .to_owned(),
        policy: owner.policy(),
        cancelled,
        continuation: Some(recovery.check),
      };
      scope.check()?;
      let snapshots = fs::directory(&scope.root, c"snapshots")?;
      let binding = Binding {
        machine: model::native::machine_identity(&recovery.boot.machine_id)
          .context("Invalid recovery VM")?,
        snapshot: prior,
        configuration: recovery.configuration,
      };
      let source = Package::inspect(&snapshots, binding.machine, prior, || scope.check())?;
      let (_, devices, _) = hardware::checkpoint(
        recovery.boot.clone(),
        &source,
        binding.configuration,
        || scope.check(),
      )?;
      let saved = checkpoint::load(&source, &devices, || scope.check())?;
      // an independent copy of the valid prior supplies rollback; the damaged target is never admitted.
      let backup = *uuid::Uuid::new_v4().as_bytes();
      let mut sealed = checkpoint::stage(
        &saved,
        &snapshots,
        Binding {
          snapshot: backup,
          ..binding
        },
        || scope.check(),
      )?;
      let description = Description::new(
        "Before recovery",
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
      )?;
      sealed.describe(&description, || scope.check())?;
      drop(sealed.publish(|| scope.check(), || owner.commit())?);
      drop(saved);
      drop(devices);
      scope.check()?;
      ensure!(
        owner.selected()? == selected,
        "Recovery selection changed during backup"
      );
      let (boot, restore) = stage::prepare(
        &mut owner,
        recovery.boot,
        prior,
        backup,
        selected,
        scope,
        Some(recovery.configuration),
      )?;
      Ok(Prepared {
        owner,
        boot,
        restore,
      })
    })();
    let outcome = match prepared {
      Ok(prepared) => Ok(Recovered { prepared, failure }),
      Err(error) => Err(failure.context(format!("Restore preparation failed; prior recovery did not complete; retained checkpoints require recovery: {error:#}"))),
    };
    let _ = completion.send(outcome);
  });
  result
    .await
    .context("Preparation recovery owner ended without completion")?
}
