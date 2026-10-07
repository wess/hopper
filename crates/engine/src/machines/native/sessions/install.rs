use super::{
  super::{assets::Assets, config, deployment::Phase, installation},
  owner, Registry, Sessions,
};
use crate::machines::Actor;
use anyhow::{ensure, Context};
use model::native::{Command, Installation, Result as Reply, SetupStatus};
use std::{
  sync::{Arc, Weak},
  time::Duration,
};
use tokio::sync::watch;

pub(super) fn monitor(
  registry: Weak<Registry>,
  id: String,
  assets: Assets,
  progress: watch::Sender<Phase>,
  generation: Arc<std::sync::Mutex<u64>>,
  epoch: u64,
) {
  tokio::spawn(async move {
    let mut ticks = tokio::time::interval(Duration::from_millis(500));
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
      ticks.tick().await;
      if *generation.lock().unwrap_or_else(|error| error.into_inner()) != epoch {
        return;
      }
      let Some(inner) = registry.upgrade() else {
        return;
      };
      let owner = Sessions { inner };
      let result = owner
        .installation_tick(&id, &assets, &progress, epoch)
        .await;
      drop(owner);
      match result {
        Ok(true) => {}
        Ok(false) => return,
        Err(error)
          if error
            .chain()
            .filter_map(|error| error.downcast_ref::<std::io::Error>())
            .any(|error| error.kind() == std::io::ErrorKind::WouldBlock) => {}
        Err(error) => {
          tracing::warn!("Native installation supervision stopped for {id}: {error:#}");
          return;
        }
      }
    }
  });
}

impl Sessions {
  pub fn installation(&self, id: &str, actor: Actor) -> anyhow::Result<Option<Installation>> {
    installation::read(&self.inner.manager, id, actor)
  }

  pub async fn start_windows(
    &self,
    id: &str,
    progress: watch::Sender<Phase>,
  ) -> anyhow::Result<()> {
    let current = self.installation(id, Actor::Person)?;
    match installation::boot_stage(current.as_ref())? {
      config::Stage::Deployment => self.deploy_bundled(id, progress).await,
      config::Stage::System => {
        progress.send_replace(Phase::SystemBoot);
        let assets = super::super::assets::locate()?;
        self
          .start_configured(id, &assets, config::Stage::System)
          .await
      }
    }
  }

  async fn installation_tick(
    &self,
    id: &str,
    assets: &Assets,
    progress: &watch::Sender<Phase>,
    epoch: u64,
  ) -> anyhow::Result<bool> {
    let slot = self.slot(id).await?;
    let mut session = slot.session.clone().lock_owned().await;
    let lease = self.inner.manager.lock(id)?;
    let manager = self.inner.manager.clone();
    let id = id.to_owned();
    let assets = assets.clone();
    let progress = progress.clone();
    let (send, receive) = tokio::sync::oneshot::channel();
    // an observed completion finishes its handoff under the same operation lease,
    // even if its caller disappears between stopping WinPE and starting the system.
    tokio::spawn(async move {
      let _lease = lease;
      let result = async {
        if *slot
          .generation
          .lock()
          .unwrap_or_else(|error| error.into_inner())
          != epoch
        {
          return Ok(false);
        }
        let previous = installation::read(&manager, &id, Actor::Person)?
          .context("Missing installation record")?;
        let Some(active) = session.as_ref() else {
          return Ok(false);
        };
        if matches!(
          super::super::state(&active.client),
          super::super::State::Stopped(_) | super::super::State::Failed(_)
        ) {
          owner::finished(active).await?;
          installation::interrupted(&manager, &id)?;
          return Ok(false);
        }
        let (paused, setup) =
          match super::super::request(&active.client, Command::Status {}).await? {
            Reply::Status { paused, setup } => (paused, setup),
            Reply::Stopped { .. } => {
              owner::finished(active).await?;
              installation::interrupted(&manager, &id)?;
              return Ok(false);
            }
            _ => anyhow::bail!("Worker did not return installation status"),
          };
        let next = installation::observed(&previous, setup)?;
        if next != previous {
          installation::save(&manager, &id, next.clone())?;
        }
        progress.send_replace(Phase::Installing(setup));
        if !matches!(next, Installation::Deployed {}) || paused {
          return Ok(matches!(
            next,
            Installation::Setup {
              status: SetupStatus::Waiting {} | SetupStatus::Active { .. }
            } | Installation::Deployed {}
          ));
        }
        let stopped = super::super::request(&active.client, Command::Stop {}).await?;
        ensure!(
          matches!(stopped, Reply::Stopped { .. }),
          "Installer worker did not stop"
        );
        owner::finished(active).await?;
        owner::retire(&slot, super::super::state(&active.client));
        session.take();
        installation::save(&manager, &id, Installation::Booting {})?;
        progress.send_replace(Phase::SystemBoot);
        let boot = config::prepare(&manager, &id, &assets, config::Stage::System)?;
        ensure!(
          boot.boot_media.is_none() && boot.installer.is_none(),
          "System boot retained installation media"
        );
        let runtime = manager.guard(&id, ".runtime")?;
        let client = super::super::launch(&assets.worker, boot).await?;
        let created = owner::started(
          &manager,
          &id,
          client,
          runtime,
          Some(Installation::SystemStarted {}),
        )
        .await?;
        owner::publish(&slot, &created.client);
        *session = Some(created);
        Ok(false)
      }
      .await;
      if result.is_err() {
        if matches!(
          installation::read(&manager, &id, Actor::Person),
          Ok(Some(Installation::Booting {}))
        ) {
          let _ = installation::save(&manager, &id, Installation::HandoffFailed {});
        } else {
          let _ = installation::interrupted(&manager, &id);
        }
      }
      let _ = send.send(result);
    });
    receive.await.context("Installation supervision ended")?
  }
}
