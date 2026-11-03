//! Internal worker transport. VM authorization belongs to the manager before
//! forwarding operations. A worker and its boot paths are never an agent endpoint.

mod actor;
pub mod assets;
pub mod config;
pub mod deployment;
pub mod installation;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub mod restore;
pub mod sessions;
pub mod snapshot;
mod startup;
mod wire;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub use restore::{
  launch_generation, launch_worker as launch_restored, pause_worker as pause_restored,
  resume_generation, resume_worker as resume_restored,
};

use anyhow::{bail, Context};
use model::native::{Boot, Command, Result as Reply, StopReason};
use std::{path::Path, sync::Arc, time::Duration};
use tokio::sync::{mpsc, oneshot, watch};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
  Running,
  Paused,
  Stopped(StopReason),
  Failed(String),
}

#[derive(Clone)]
pub struct Client {
  configuration: [u8; 32],
  #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
  hardware: Option<snapshot::Hardware>,
  requests: mpsc::Sender<Pending>,
  state: watch::Receiver<State>,
  ended: watch::Receiver<bool>,
}

type Check = Arc<dyn Fn() -> anyhow::Result<()> + Send + Sync>;

struct Pending {
  command: Command,
  response: oneshot::Sender<anyhow::Result<wire::Packet>>,
  check: Option<Check>,
}

pub struct Frame {
  pub width: u32,
  pub height: u32,
  pub generation: u64,
  pub rgba: Vec<u8>,
}

pub async fn launch(helper: &Path, boot: Boot) -> anyhow::Result<Client> {
  startup::launch(helper, boot, startup::Kind::Normal, None).await
}

pub fn state(client: &Client) -> State {
  client.state.borrow().clone()
}

pub async fn changed(client: &mut Client) -> anyhow::Result<State> {
  client
    .state
    .changed()
    .await
    .context("Native worker state stream ended")?;
  Ok(state(client))
}

pub async fn request(client: &Client, command: Command) -> anyhow::Result<Reply> {
  Ok(transact(client, command, None).await?.response.result)
}

pub async fn finished(client: &Client) -> anyhow::Result<()> {
  let mut ended = client.ended.clone();
  while !*ended.borrow_and_update() {
    ended
      .changed()
      .await
      .context("Native worker cleanup failed")?;
  }
  Ok(())
}

pub async fn capture(client: &Client) -> anyhow::Result<Frame> {
  frame(transact(client, Command::Capture {}, None).await?)
}

fn frame(packet: wire::Packet) -> anyhow::Result<Frame> {
  let Reply::Frame {
    width,
    height,
    generation,
  } = packet.response.result
  else {
    bail!("Native worker returned no frame");
  };
  Ok(Frame {
    width,
    height,
    generation,
    rgba: packet.pixels,
  })
}

async fn transact(
  client: &Client,
  command: Command,
  check: Option<Check>,
) -> anyhow::Result<wire::Packet> {
  if matches!(
    command,
    Command::Start { .. } | Command::StartRestored { .. } | Command::StartGeneration { .. }
  ) {
    bail!("Boot configuration is only accepted when launching a native worker");
  }
  let limit = if matches!(command, Command::Capture {}) {
    150
  } else {
    40
  };
  tokio::time::timeout(Duration::from_secs(limit), async {
    let (response, reply) = oneshot::channel();
    client
      .requests
      .send(Pending {
        command,
        response,
        check,
      })
      .await
      .map_err(|_| anyhow::anyhow!("Native VM is no longer running"))?;
    let packet = reply.await.context("Native VM stopped before replying")??;
    if let Reply::Rejected { message } = &packet.response.result {
      bail!("Native command rejected: {message}");
    }
    Ok(packet)
  })
  .await
  .context("Native VM request timed out")?
}

/// Hardware fingerprint frozen by this worker before guest execution.
pub fn configuration(client: &Client) -> [u8; 32] {
  client.configuration
}
