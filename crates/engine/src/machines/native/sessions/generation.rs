use super::{owner, Registry, Slot};
use crate::machines::{
  native::{
    self,
    assets::Assets,
    config,
    restore::{Owner, Record},
    snapshot::Policy,
    State,
  },
  Actor, Machines,
};
use anyhow::{ensure, Context as _};
use model::native::{Boot, Command, Restore, StopReason};
use std::{
  path::PathBuf,
  sync::{Arc, Weak},
  time::Duration,
};

pub(super) struct Context {
  pub manager: Machines,
  pub id: String,
  pub selected: Record,
  pub policy: Policy,
  pub namespace: (u64, u64),
  pub preparation: Preparation,
}

pub(super) enum Preparation {
  Configured(Assets),
  Prepared { helper: PathBuf, boot: Boot },
}

fn epoch(slot: &Slot) -> u64 {
  *slot
    .generation
    .lock()
    .unwrap_or_else(|error| error.into_inner())
}

async fn closed(registry: &Weak<Registry>) {
  while registry.strong_count() != 0 {
    tokio::time::sleep(Duration::from_millis(50)).await;
  }
}

pub(super) fn monitor(
  registry: Weak<Registry>,
  slot: Arc<Slot>,
  context: Context,
  mut expected: u64,
) {
  tokio::spawn(async move {
    let mut ticks = tokio::time::interval(Duration::from_millis(100));
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
      tokio::select! {
        _ = ticks.tick() => {},
        _ = closed(&registry) => return,
      }
      if epoch(&slot) != expected {
        return;
      }
      let result = tick(&registry, &slot, &context, expected).await;
      match result {
        Ok(Some(next)) => expected = next,
        Ok(None) => return,
        Err(error)
          if error
            .chain()
            .filter_map(|error| error.downcast_ref::<std::io::Error>())
            .any(|error| error.kind() == std::io::ErrorKind::WouldBlock) => {}
        Err(error) => {
          tracing::warn!(
            "Active generation reboot stopped for {}: {error:#}",
            context.id
          );
          return;
        }
      }
    }
  });
}

async fn tick(
  registry: &Weak<Registry>,
  slot: &Arc<Slot>,
  context: &Context,
  expected: u64,
) -> anyhow::Result<Option<u64>> {
  let mut session = slot.session.clone().lock_owned().await;
  if epoch(slot) != expected || registry.strong_count() == 0 {
    return Ok(None);
  }
  let Some(previous) = session.as_ref() else {
    return Ok(None);
  };
  match native::state(&previous.client) {
    State::Running | State::Paused => return Ok(Some(expected)),
    State::Stopped(StopReason::Reset) => {}
    State::Stopped(_) | State::Failed(_) => return Ok(None),
  }
  // keep the slot locked through reaping, fresh admission and publication; stop cannot slip between.
  owner::finished(previous).await?;
  context.policy.check()?;
  let owned = Owner::continuing(
    context.manager.clone(),
    &context.id,
    context.policy.clone(),
    context.namespace,
  )?;
  ensure!(
    owned.namespace_stamp()? == context.namespace,
    "Reboot VM namespace changed"
  );
  ensure!(
    owned.selected()? == Some(context.selected),
    "Reboot active selection changed"
  );
  let (helper, boot) = match &context.preparation {
    Preparation::Configured(assets) => (
      assets.worker.clone(),
      config::prepare_generation(&context.manager, &context.id, assets, &owned)?,
    ),
    Preparation::Prepared { helper, boot } => {
      owned.boot(boot)?;
      ensure!(
        boot.boot_media.is_none() && boot.installer.is_none(),
        "Generation reboot requires detached installation media"
      );
      (helper.clone(), boot.clone())
    }
  };
  let restore = Restore {
    snapshot_id: uuid::Uuid::from_bytes(context.selected.snapshot).to_string(),
    generation_id: uuid::Uuid::from_bytes(context.selected.generation).to_string(),
    policy_generation: context
      .manager
      .machine(&context.id, Actor::Person)?
      .agent_generation,
  };
  let owned = Arc::new(owned);
  let client = tokio::select! {
    result = native::launch_generation(&helper, boot, restore.clone(), owned.clone()) => result?,
    _ = closed(registry) => return Ok(None),
  };
  let resumed = tokio::select! {
    result = native::resume_generation(&client, restore, owned.clone()) => result,
    _ = closed(registry) => Err(anyhow::anyhow!("Reboot registry closed")),
  };
  let admitted = resumed.and_then(|()| {
    let _commit = owned.commit()?;
    ensure!(
      registry.strong_count() != 0 && epoch(slot) == expected,
      "Reboot owner changed"
    );
    ensure!(
      owned.namespace_stamp()? == context.namespace,
      "Reboot VM namespace changed"
    );
    ensure!(
      owned.selected()? == Some(context.selected),
      "Reboot active selection changed"
    );
    owned.release_operation()?;
    let created = owner::restored(client.clone(), Arc::downgrade(&owned));
    let next = owner::publish(slot, &client);
    *session = Some(created);
    Ok(next)
  });
  if admitted.is_err() {
    let _ = native::request(&client, Command::Stop {}).await;
    native::finished(&client)
      .await
      .context("Reap rejected generation reboot")?;
  }
  admitted.map(Some)
}
