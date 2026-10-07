//! Host-owned native sessions. Trusted startup is separate from guest operations.

use super::{Client, Frame, State};
use crate::machines::{Actor, Machines};
use anyhow::{ensure, Context};
use model::native::{Boot, Command, Result as Reply};
use std::{collections::HashMap, path::Path, sync::Arc};
use tokio::sync::{watch, Mutex};

struct Session {
  client: Client,
  released: watch::Receiver<bool>,
}

type Slot = Arc<Mutex<Option<Session>>>;

async fn finished(session: &Session) -> anyhow::Result<()> {
  super::finished(&session.client).await?;
  let mut released = session.released.clone();
  while !*released.borrow_and_update() {
    released
      .changed()
      .await
      .context("Native ownership lease cleanup failed")?;
  }
  Ok(())
}

pub struct Sessions {
  manager: Machines,
  slots: Mutex<HashMap<String, Slot>>,
}

impl Sessions {
  pub fn new(manager: Machines) -> Self {
    Self {
      manager,
      slots: Mutex::new(HashMap::new()),
    }
  }

  async fn slot(&self, id: &str) -> anyhow::Result<Slot> {
    crate::machines::validate_id(id)?;
    Ok(
      self
        .slots
        .lock()
        .await
        .entry(id.to_owned())
        .or_default()
        .clone(),
    )
  }

  /// internal startup only; never expose helper or boot paths through an agent endpoint.
  pub async fn start(&self, id: &str, helper: &Path, boot: Boot) -> anyhow::Result<()> {
    self.start_using(id, helper, async move { Ok(boot) }).await
  }

  pub async fn start_configured(
    &self,
    id: &str,
    assets: &super::assets::Assets,
    stage: super::config::Stage,
  ) -> anyhow::Result<()> {
    let manager = self.manager.clone();
    let identity = id.to_owned();
    let configuration = assets.clone();
    self
      .start_using(id, &assets.worker, async move {
        super::config::prepare(&manager, &identity, &configuration, stage)
      })
      .await
  }

  pub async fn deploy(
    &self,
    id: &str,
    assets: &super::assets::Assets,
    tools: &super::deployment::Tools,
    progress: watch::Sender<super::deployment::Phase>,
  ) -> anyhow::Result<()> {
    let manager = self.manager.clone();
    let identity = id.to_owned();
    let configuration = assets.clone();
    let tools = tools.clone();
    self
      .start_using(id, &assets.worker, async move {
        super::deployment::prepare(&manager, &identity, &configuration, &tools, &progress).await
      })
      .await
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
  ) -> anyhow::Result<()> {
    let slot = self.slot(id).await?;
    let mut session = slot.lock().await;
    let _operation = self.manager.lock(id)?;
    let machine = self.manager.machine(id, Actor::Person)?;
    ensure!(
      machine.guest == model::GuestOs::Windows,
      "Native worker requires a Windows VM"
    );
    ensure!(
      !self.manager.root.join("lima").join(id).exists(),
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
    let lease = self.manager.guard(id, ".runtime")?;
    let helper = helper.to_owned();
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
        Ok(client) => {
          let mut ended = client.ended.clone();
          let (release, released) = watch::channel(false);
          tokio::spawn(async move {
            while !*ended.borrow_and_update() {
              if ended.changed().await.is_err() {
                break;
              }
            }
            drop(lease);
            release.send_replace(true);
          });
          Ok(Session { client, released })
        }
        Err(error) => {
          drop(lease);
          Err(error)
        }
      };
      let _ = result.send(startup);
    });
    *session = Some(started.await.context("Native session startup ended")??);
    Ok(())
  }

  pub async fn state(&self, id: &str, actor: Actor) -> anyhow::Result<Option<State>> {
    let slot = self.slot(id).await?;
    let session = slot.lock().await;
    self.manager.machine(id, actor)?;
    Ok(
      session
        .as_ref()
        .map(|session| super::state(&session.client)),
    )
  }

  /// viewer watches contain state only and cannot keep the worker running.
  pub async fn watch(&self, id: &str) -> anyhow::Result<watch::Receiver<State>> {
    let slot = self.slot(id).await?;
    let session = slot.lock().await;
    self.manager.machine(id, Actor::Person)?;
    Ok(
      session
        .as_ref()
        .context("VM is not running")?
        .client
        .state
        .clone(),
    )
  }

  pub async fn request(&self, id: &str, actor: Actor, command: Command) -> anyhow::Result<Reply> {
    ensure!(
      !matches!(
        command,
        Command::Start { .. } | Command::Capture {} | Command::Stop {}
      ),
      "Use the dedicated native lifecycle or capture operation"
    );
    let slot = self.slot(id).await?;
    let session = slot.lock().await;
    let _operation = self.manager.lock(id)?;
    let check = self.check(id, actor);
    check()?;
    let active = session.as_ref().context("VM is not running")?;
    let client = &active.client;
    Ok(
      super::transact(client, command, Some(check))
        .await?
        .response
        .result,
    )
  }

  pub async fn capture(&self, id: &str, actor: Actor) -> anyhow::Result<Frame> {
    let slot = self.slot(id).await?;
    let session = slot.lock().await;
    let _operation = self.manager.lock(id)?;
    let check = self.check(id, actor);
    check()?;
    let active = session.as_ref().context("VM is not running")?;
    let client = &active.client;
    super::frame(super::transact(client, Command::Capture {}, Some(check)).await?)
  }

  pub async fn stop(&self, id: &str, actor: Actor) -> anyhow::Result<()> {
    let slot = self.slot(id).await?;
    let mut session = slot.lock().await;
    let _operation = self.manager.lock(id)?;
    let check = self.check(id, actor);
    check()?;
    let active = session.as_ref().context("VM is not running")?;
    let client = &active.client;
    if matches!(super::state(client), State::Running | State::Paused) {
      let reply = super::transact(client, Command::Stop {}, Some(check))
        .await?
        .response
        .result;
      ensure!(
        matches!(reply, Reply::Stopped { .. }),
        "Native worker did not stop"
      );
    }
    finished(active).await?;
    session.take();
    Ok(())
  }

  fn check(&self, id: &str, actor: Actor) -> super::Check {
    let manager = self.manager.clone();
    let id = id.to_owned();
    Arc::new(move || {
      manager.machine(&id, actor)?;
      Ok(())
    })
  }
}
