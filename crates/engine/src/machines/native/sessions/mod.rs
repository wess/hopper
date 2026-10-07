//! Host-owned native sessions. Trusted startup is separate from guest operations.

use super::State;
use crate::machines::{Actor, Machines};
use anyhow::{ensure, Context};
use model::native::Boot;
use std::{collections::HashMap, path::Path, sync::Arc};
use tokio::sync::{watch, Mutex};

mod guest;
mod install;
mod owner;
use owner::{finished, Session};

struct Slot {
  session: Arc<Mutex<Option<Session>>>,
  state: watch::Sender<State>,
  generation: Arc<std::sync::Mutex<u64>>,
}

struct Registry {
  manager: Machines,
  slots: Mutex<HashMap<String, Arc<Slot>>>,
}

pub struct Sessions {
  inner: Arc<Registry>,
}

impl Sessions {
  pub fn new(manager: Machines) -> Self {
    Self {
      inner: Arc::new(Registry {
        manager,
        slots: Mutex::new(HashMap::new()),
      }),
    }
  }

  async fn slot(&self, id: &str) -> anyhow::Result<Arc<Slot>> {
    crate::machines::validate_id(id)?;
    Ok(
      self
        .inner
        .slots
        .lock()
        .await
        .entry(id.to_owned())
        .or_insert_with(|| {
          let (state, _) = watch::channel(State::Stopped(model::native::StopReason::Requested));
          Arc::new(Slot {
            session: Arc::new(Mutex::new(None)),
            state,
            generation: Arc::new(std::sync::Mutex::new(0)),
          })
        })
        .clone(),
    )
  }

  /// internal startup only; never expose helper or boot paths through an agent endpoint.
  pub async fn start(&self, id: &str, helper: &Path, boot: Boot) -> anyhow::Result<()> {
    self
      .start_using(id, helper, async move { Ok(boot) }, None)
      .await
      .map(|_| ())
  }

  pub async fn start_configured(
    &self,
    id: &str,
    assets: &super::assets::Assets,
    stage: super::config::Stage,
  ) -> anyhow::Result<()> {
    let manager = self.inner.manager.clone();
    let identity = id.to_owned();
    let configuration = assets.clone();
    let current = super::installation::read(&manager, id, Actor::Person)?;
    let system = matches!(stage, super::config::Stage::System) && current.is_some();
    if system {
      ensure!(
        matches!(
          super::installation::boot_stage(current.as_ref())?,
          super::config::Stage::System
        ),
        "Windows deployment is not complete"
      );
    }
    self
      .start_using(
        id,
        &assets.worker,
        async move {
          let boot = super::config::prepare(&manager, &identity, &configuration, stage)?;
          if system {
            super::installation::save(
              &manager,
              &identity,
              model::native::Installation::Booting {},
            )?;
          }
          Ok(boot)
        },
        system.then_some(model::native::Installation::SystemStarted {}),
      )
      .await
      .map(|_| ())
  }

  pub async fn deploy(
    &self,
    id: &str,
    assets: &super::assets::Assets,
    tools: &super::deployment::Tools,
    progress: watch::Sender<super::deployment::Phase>,
  ) -> anyhow::Result<()> {
    let manager = self.inner.manager.clone();
    let identity = id.to_owned();
    let configuration = assets.clone();
    let tools = tools.clone();
    let report = progress.clone();
    let epoch = self
      .start_using(
        id,
        &assets.worker,
        async move {
          super::deployment::prepare(&manager, &identity, &configuration, &tools, &progress).await
        },
        None,
      )
      .await?;
    let slot = self.slot(id).await?;
    install::monitor(
      Arc::downgrade(&self.inner),
      id.to_owned(),
      assets.clone(),
      report,
      slot.generation.clone(),
      epoch,
    );
    Ok(())
  }

  pub async fn deploy_bundled(
    &self,
    id: &str,
    progress: watch::Sender<super::deployment::Phase>,
  ) -> anyhow::Result<()> {
    let assets = super::assets::locate()?;
    let tools = crate::machines::windows::assets::locate()?;
    self.deploy(id, &assets, &tools, progress).await
  }

  async fn start_using(
    &self,
    id: &str,
    helper: &Path,
    prepare: impl std::future::Future<Output = anyhow::Result<Boot>> + Send + 'static,
    completion: Option<model::native::Installation>,
  ) -> anyhow::Result<u64> {
    let slot = self.slot(id).await?;
    let mut session = slot.session.lock().await;
    let _operation = self.inner.manager.lock(id)?;
    let machine = self.inner.manager.machine(id, Actor::Person)?;
    ensure!(
      machine.guest == model::GuestOs::Windows,
      "Native worker requires a Windows VM"
    );
    ensure!(
      !self.inner.manager.root.join("lima").join(id).exists(),
      "This VM needs migration from the previous runtime before native startup"
    );
    if let Some(previous) = session.as_ref() {
      ensure!(
        matches!(
          super::state(&previous.client),
          State::Stopped(_) | State::Failed(_)
        ),
        "VM is already running"
      );
      finished(previous).await?;
      session.take();
    }
    let lease = self.inner.manager.guard(id, ".runtime")?;
    let helper = helper.to_owned();
    let manager = self.inner.manager.clone();
    let identity = id.to_owned();
    let (result, started) = tokio::sync::oneshot::channel();
    // cancellation drops the result, while startup still cleans up under both locks.
    tokio::spawn(async move {
      let _operation = _operation;
      let prepared = async {
        let boot = prepare.await?;
        ensure!(
          !result.is_closed(),
          "Native startup caller was cancelled; prepared data retained"
        );
        super::launch(&helper, boot).await
      }
      .await;
      let startup = match prepared {
        Ok(client) => owner::started(&manager, &identity, client, lease, completion).await,
        Err(error) => {
          drop(lease);
          Err(error)
        }
      };
      let _ = result.send(startup);
    });
    let created = started.await.context("Native session startup ended")??;
    let epoch = owner::publish(&slot, &created.client);
    *session = Some(created);
    Ok(epoch)
  }

  pub async fn state(&self, id: &str, actor: Actor) -> anyhow::Result<Option<State>> {
    let slot = self.slot(id).await?;
    let session = slot.session.lock().await;
    self.inner.manager.machine(id, actor)?;
    Ok(
      session
        .as_ref()
        .map(|session| super::state(&session.client)),
    )
  }

  /// viewer watches contain state only and cannot keep the worker running.
  pub async fn watch(&self, id: &str) -> anyhow::Result<watch::Receiver<State>> {
    let slot = self.slot(id).await?;
    let session = slot.session.lock().await;
    self.inner.manager.machine(id, Actor::Person)?;
    session.as_ref().context("VM is not running")?;
    Ok(slot.state.subscribe())
  }
}
