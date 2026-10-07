mod server;

use super::{wire, Operation, Reply};
use crate::machines::{Actor, Machines};
use anyhow::{ensure, Context};
use model::native::{InputDevice, InputEvent};
use serde::{Deserialize, Serialize};
pub(super) use server::serve;
use std::time::Duration;
use tokio::net::UnixStream;

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
enum Message {
  Input {
    device: InputDevice,
    events: Vec<InputEvent>,
  },
  Release {},
}

pub struct Control {
  stream: Option<UnixStream>,
  manager: Machines,
  id: String,
  policy: u64,
}

pub async fn connect(manager: &Machines, id: &str) -> anyhow::Result<Control> {
  let policy = manager.machine(id, Actor::Agent)?.agent_generation;
  let stream = tokio::time::timeout(Duration::from_secs(45), async {
    let mut stream = super::connect(manager, id, Operation::Control, policy).await?;
    let reply: Reply = wire::read(&mut stream).await?;
    match reply {
      Reply::Owned {} => Ok(stream),
      Reply::Error { message } => anyhow::bail!("{message}"),
      _ => anyhow::bail!("Unexpected native input ownership reply"),
    }
  })
  .await
  .context("Native input connection timed out")??;
  ensure!(
    manager.machine(id, Actor::Agent)?.agent_generation == policy,
    "VM agent policy has changed"
  );
  Ok(Control {
    stream: Some(stream),
    manager: manager.clone(),
    id: id.into(),
    policy,
  })
}

impl Control {
  pub async fn send(&mut self, device: InputDevice, events: Vec<InputEvent>) -> anyhow::Result<()> {
    ensure!(
      !events.is_empty() && events.len() <= 64,
      "Native remote input batches require 1–64 events"
    );
    self
      .exchange(Message::Input { device, events }, false)
      .await
  }

  pub async fn close(mut self) -> anyhow::Result<()> {
    self.exchange(Message::Release {}, true).await
  }

  async fn exchange(&mut self, message: Message, closing: bool) -> anyhow::Result<()> {
    // taking the stream makes cancellation close the connection and its input lease.
    let mut stream = self
      .stream
      .take()
      .context("Native input connection is closed")?;
    ensure!(
      self
        .manager
        .machine(&self.id, Actor::Agent)?
        .agent_generation
        == self.policy,
      "VM agent policy has changed"
    );
    tokio::time::timeout(Duration::from_secs(45), async {
      wire::write(&mut stream, &message, &[]).await?;
      let reply: Reply = wire::read(&mut stream).await?;
      match reply {
        Reply::Accepted {} if !closing => Ok(()),
        Reply::Released {} if closing => Ok(()),
        Reply::Error { message } => anyhow::bail!("{message}"),
        _ => anyhow::bail!("Unexpected native input reply"),
      }
    })
    .await
    .context("Native input request timed out")??;
    ensure!(
      self
        .manager
        .machine(&self.id, Actor::Agent)?
        .agent_generation
        == self.policy,
      "VM agent policy has changed"
    );
    if !closing {
      self.stream = Some(stream);
    }
    Ok(())
  }
}
