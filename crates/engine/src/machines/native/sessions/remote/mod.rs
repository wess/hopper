//! Same-user agent transport to the registry that owns native workers.

pub mod control;
mod path;
mod server;
mod wire;

use super::Sessions;
use crate::machines::{Actor, Machines};
use anyhow::{ensure, Context};
use model::native::StopReason;
use serde::{Deserialize, Serialize};
pub use server::Server;
use std::time::Duration;
use tokio::net::UnixStream;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
  vm_id: String,
  operation: Operation,
  #[serde(default)]
  agent_generation: Option<u64>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum Operation {
  Status,
  Capture,
  Control,
  Pause,
  Resume,
  Stop,
}

#[derive(Clone, Copy)]
pub enum Lifecycle {
  Pause,
  Resume,
  Stop,
}

pub async fn lifecycle(manager: &Machines, id: &str, action: Lifecycle) -> anyhow::Result<()> {
  let operation = match action {
    Lifecycle::Pause => Operation::Pause,
    Lifecycle::Resume => Operation::Resume,
    Lifecycle::Stop => Operation::Stop,
  };
  let (reply, _) = exchange(manager, id, operation).await?;
  ensure!(
    matches!(
      (action, reply),
      (
        Lifecycle::Pause,
        Reply::State {
          state: Some(Status::Paused {})
        }
      ) | (
        Lifecycle::Resume,
        Reply::State {
          state: Some(Status::Running {})
        }
      ) | (
        Lifecycle::Stop,
        Reply::State {
          state: Some(Status::Stopped { .. })
        }
      )
    ),
    "Unexpected native lifecycle reply"
  );
  Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
enum Reply {
  Owned {},
  Accepted {},
  Released {},
  State {
    state: Option<Status>,
  },
  Frame {
    width: u32,
    height: u32,
    generation: u64,
  },
  Error {
    message: String,
  },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum Status {
  Running {},
  Paused {},
  Stopped { reason: StopReason },
  Failed { message: String },
}

impl Sessions {
  pub fn serve_agents(&self) -> anyhow::Result<Server> {
    server::start(&self.inner)
  }
}

async fn exchange(
  manager: &Machines,
  id: &str,
  operation: Operation,
) -> anyhow::Result<(Reply, Vec<u8>)> {
  let policy = manager.machine(id, Actor::Agent)?.agent_generation;
  let response = tokio::time::timeout(Duration::from_secs(165), async {
    let mut stream = connect(manager, id, operation, policy).await?;
    let reply: Reply = wire::read(&mut stream).await?;
    let size = match &reply {
      Reply::Frame { width, height, .. } => wire::frame_size(*width, *height)?,
      Reply::Error { message } => anyhow::bail!("{message}"),
      _ => 0,
    };
    let mut pixels = vec![0; size];
    use tokio::io::AsyncReadExt;
    stream.read_exact(&mut pixels).await?;
    Ok::<_, anyhow::Error>((reply, pixels))
  })
  .await
  .context("Native VM service timed out")??;
  ensure!(
    manager.machine(id, Actor::Agent)?.agent_generation == policy,
    "VM agent policy has changed"
  );
  Ok(response)
}

async fn connect(
  manager: &Machines,
  id: &str,
  operation: Operation,
  policy: u64,
) -> anyhow::Result<UnixStream> {
  ensure!(
    manager.machine(id, Actor::Agent)?.agent_generation == policy,
    "VM agent policy has changed"
  );
  let endpoint = path::endpoint(manager)?;
  path::socket(&endpoint)?;
  let mut stream = UnixStream::connect(endpoint)
    .await
    .context("Connect to Hopper's native VM service")?;
  ensure!(
    stream.peer_cred()?.uid() == path::uid(),
    "Native VM service belongs to another user"
  );
  wire::write(
    &mut stream,
    &Request {
      vm_id: id.into(),
      operation,
      agent_generation: Some(policy),
    },
    &[],
  )
  .await?;
  Ok(stream)
}

pub async fn status(manager: &Machines, id: &str) -> anyhow::Result<Option<Status>> {
  let (reply, _) = exchange(manager, id, Operation::Status).await?;
  let Reply::State { state } = reply else {
    anyhow::bail!("Unexpected native status reply");
  };
  Ok(state)
}

pub async fn capture(manager: &Machines, id: &str) -> anyhow::Result<super::super::Frame> {
  let (reply, rgba) = exchange(manager, id, Operation::Capture).await?;
  let Reply::Frame {
    width,
    height,
    generation,
  } = reply
  else {
    anyhow::bail!("Unexpected native frame reply");
  };
  Ok(super::super::Frame {
    width,
    height,
    generation,
    rgba,
  })
}
