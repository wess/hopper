//! Internal worker transport. VM authorization belongs to the manager before
//! forwarding operations. A worker and its boot paths are never an agent endpoint.

mod actor;
pub mod assets;
pub mod config;
pub mod deployment;
pub mod installation;
pub mod sessions;
mod wire;

use anyhow::{bail, Context};
use model::native::{Boot, Command, Result as Reply, StopReason};
use std::{path::Path, process::Stdio, sync::Arc, time::Duration};
use tokio::{
  process,
  sync::{mpsc, oneshot, watch},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
  Running,
  Paused,
  Stopped(StopReason),
  Failed(String),
}

#[derive(Clone)]
pub struct Client {
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
  let mut child = process::Command::new(helper)
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::null())
    .kill_on_drop(true)
    .spawn()
    .context("Launch native VM worker")?;
  let mut input = child.stdin.take().context("Missing native worker input")?;
  let mut output = child
    .stdout
    .take()
    .context("Missing native worker output")?;
  let (packets, mut replies) = mpsc::channel(2);
  let reader = tokio::spawn(async move {
    loop {
      let packet = wire::read(&mut output).await;
      let ended = packet.is_err();
      if packets.send(packet).await.is_err() || ended {
        break;
      }
    }
  });
  let reader = actor::Reader(reader.abort_handle());
  let command = Command::Start { boot };
  let startup = async {
    let packet = actor::exchange(
      &mut input,
      &mut replies,
      1,
      command,
      Duration::from_secs(30),
    )
    .await?;
    if let Reply::Rejected { message } = packet.response.result {
      bail!("Native worker rejected startup: {message}");
    }
    anyhow::ensure!(
      matches!(packet.response.result, Reply::Started {}),
      "Native worker stopped during startup"
    );
    Ok::<_, anyhow::Error>(())
  }
  .await;
  if let Err(error) = startup {
    let _ = child.kill().await;
    let _ = child.wait().await;
    return Err(error);
  }
  let (requests, queue) = mpsc::channel(8);
  let (state, status) = watch::channel(State::Running);
  let (finished, ended) = watch::channel(false);
  tokio::spawn(actor::serve(
    child, input, replies, queue, state, reader, finished,
  ));
  Ok(Client {
    requests,
    state: status,
    ended,
  })
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
  if matches!(command, Command::Start { .. }) {
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
