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
    ensure!(
      !matches!(command, Command::Input { .. } | Command::Release {}),
      "Guest input requires an exclusive input lease"
    );
    self.request_using(id, actor, command, None).await
  }

  pub(super) async fn request_owned(
    &self,
    id: &str,
    actor: Actor,
    command: Command,
    owner: super::super::Check,
  ) -> anyhow::Result<Reply> {
    self.request_using(id, actor, command, Some(owner)).await
  }

  async fn request_using(
    &self,
    id: &str,
    actor: Actor,
    command: Command,
    owner: Option<super::super::Check>,
  ) -> anyhow::Result<Reply> {
    let slot = self.slot(id).await?;
    let session = slot.session.lock().await;
    let _operation = self.inner.manager.lock(id)?;
    let active = session.as_ref().context("VM is not running")?;
    let client = &active.client;
    let state = client.state.clone();
    let input = matches!(command, Command::Input { .. });
    let policy = self.check(id, actor)?;
    let check: super::super::Check = Arc::new(move || {
      policy()?;
      if let Some(owner) = &owner {
        owner()?;
      }
      if input {
        ensure!(
          matches!(*state.borrow(), State::Running),
          "Guest input requires a running VM"
        );
      }
      Ok(())
    });
    check()?;
    Ok(
      super::super::transact(client, command, Some(check))
        .await?
        .response
        .result,
    )
  }

  pub async fn capture(&self, id: &str, actor: Actor) -> anyhow::Result<Frame> {
    self.capture_using(id, actor, None).await
  }

  pub(super) async fn capture_owned(
    &self,
    id: &str,
    actor: Actor,
    policy: super::super::Check,
  ) -> anyhow::Result<Frame> {
    self.capture_using(id, actor, Some(policy)).await
  }

  async fn capture_using(
    &self,
    id: &str,
    actor: Actor,
    policy: Option<super::super::Check>,
  ) -> anyhow::Result<Frame> {
    let slot = self.slot(id).await?;
    let session = slot.session.lock().await;
    let _operation = self.inner.manager.lock(id)?;
    let check = self.scoped_check(id, actor, policy)?;
    check()?;
    let active = session.as_ref().context("VM is not running")?;
    let client = &active.client;
    super::super::frame(super::super::transact(client, Command::Capture {}, Some(check)).await?)
  }

  pub async fn stop(&self, id: &str, actor: Actor) -> anyhow::Result<()> {
    self.stop_using(id, actor, None).await
  }

  pub(super) async fn stop_owned(
    &self,
    id: &str,
    actor: Actor,
    policy: super::super::Check,
  ) -> anyhow::Result<()> {
    self.stop_using(id, actor, Some(policy)).await
  }

  async fn stop_using(
    &self,
    id: &str,
    actor: Actor,
    policy: Option<super::super::Check>,
  ) -> anyhow::Result<()> {
    let slot = self.slot(id).await?;
    let mut session = slot.session.lock().await;
    let _operation = self.inner.manager.lock(id)?;
    let check = self.scoped_check(id, actor, policy)?;
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

  fn check(&self, id: &str, actor: Actor) -> anyhow::Result<super::super::Check> {
    let policy = self.inner.manager.machine(id, actor)?.agent_generation;
    let manager = self.inner.manager.clone();
    let id = id.to_owned();
    Ok(Arc::new(move || {
      let machine = manager.machine(&id, actor)?;
      ensure!(
        actor != Actor::Agent || machine.agent_generation == policy,
        "VM agent policy has changed"
      );
      Ok(())
    }))
  }

  fn scoped_check(
    &self,
    id: &str,
    actor: Actor,
    policy: Option<super::super::Check>,
  ) -> anyhow::Result<super::super::Check> {
    let check = self.check(id, actor)?;
    Ok(Arc::new(move || {
      check()?;
      if let Some(policy) = &policy {
        policy()?;
      }
      Ok(())
    }))
  }
}
