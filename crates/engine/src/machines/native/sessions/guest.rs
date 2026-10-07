use super::super::{Frame, State};
use super::{owner, Sessions};
use crate::machines::Actor;
use anyhow::{ensure, Context};
use model::native::{Command, Result as Reply};
use std::sync::Arc;

impl Sessions {
  pub async fn request(&self, id: &str, actor: Actor, command: Command) -> anyhow::Result<Reply> {
    ensure!(
      !matches!(
        command,
        Command::Start { .. } | Command::Capture {} | Command::Stop {}
      ),
      "Use the dedicated native lifecycle or capture operation"
    );
    let slot = self.slot(id).await?;
    let session = slot.session.lock().await;
    let _operation = self.inner.manager.lock(id)?;
    let check = self.check(id, actor);
    check()?;
    let active = session.as_ref().context("VM is not running")?;
    let client = &active.client;
    Ok(
      super::super::transact(client, command, Some(check))
        .await?
        .response
        .result,
    )
  }

  pub async fn capture(&self, id: &str, actor: Actor) -> anyhow::Result<Frame> {
    let slot = self.slot(id).await?;
    let session = slot.session.lock().await;
    let _operation = self.inner.manager.lock(id)?;
    let check = self.check(id, actor);
    check()?;
    let active = session.as_ref().context("VM is not running")?;
    let client = &active.client;
    super::super::frame(super::super::transact(client, Command::Capture {}, Some(check)).await?)
  }

  pub async fn stop(&self, id: &str, actor: Actor) -> anyhow::Result<()> {
    let slot = self.slot(id).await?;
    let mut session = slot.session.lock().await;
    let _operation = self.inner.manager.lock(id)?;
    let check = self.check(id, actor);
    check()?;
    let active = session.as_ref().context("VM is not running")?;
    let client = &active.client;
    if matches!(super::super::state(client), State::Running | State::Paused) {
      let reply = super::super::transact(client, Command::Stop {}, Some(check))
        .await?
        .response
        .result;
      ensure!(
        matches!(reply, Reply::Stopped { .. }),
        "Native worker did not stop"
      );
    }
    owner::finished(active).await?;
    super::super::installation::interrupted(&self.inner.manager, id)?;
    owner::retire(&slot, super::super::state(&active.client));
    session.take();
    Ok(())
  }

  fn check(&self, id: &str, actor: Actor) -> super::super::Check {
    let manager = self.inner.manager.clone();
    let id = id.to_owned();
    Arc::new(move || {
      manager.machine(&id, actor)?;
      Ok(())
    })
  }
}
