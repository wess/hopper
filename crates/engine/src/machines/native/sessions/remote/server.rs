use super::{path, wire, Operation, Reply, Request, Status};
use crate::machines::{
  native::{
    sessions::{Registry, Sessions},
    State,
  },
  Actor,
};
use anyhow::{ensure, Context};
use std::{
  os::unix::fs::PermissionsExt,
  sync::{Arc, Weak},
  time::Duration,
};
use tokio::{
  io::{AsyncReadExt, AsyncWriteExt},
  net::{UnixListener, UnixStream},
  sync::Semaphore,
  task::{JoinHandle, JoinSet},
};

pub struct Server {
  task: JoinHandle<()>,
  _socket: path::Socket,
  _lease: store::lock::Lease,
}

impl Drop for Server {
  fn drop(&mut self) {
    self.task.abort();
  }
}

pub(super) fn start(registry: &Arc<Registry>) -> anyhow::Result<Server> {
  tokio::runtime::Handle::try_current()
    .context("Native agent service requires an async runtime")?;
  let (endpoint, lease) = path::prepare(&registry.manager)?;
  let listener = UnixListener::bind(&endpoint)?;
  std::fs::set_permissions(&endpoint, std::fs::Permissions::from_mode(0o600))?;
  let socket = path::Socket {
    identity: path::socket(&endpoint)?,
    path: endpoint,
  };
  let registry = Arc::downgrade(registry);
  let task = tokio::spawn(async move {
    let mut connections = JoinSet::new();
    let captures = Arc::new(Semaphore::new(1));
    loop {
      tokio::select! {
        result = connections.join_next(), if !connections.is_empty() => { let _ = result; }
        accepted = listener.accept(), if connections.len() < 16 => {
          let Ok((stream, _)) = accepted else { break; };
          let registry = registry.clone();
          let captures = captures.clone();
          connections.spawn(async move {
            let _ = connection(stream, registry, captures).await;
          });
        }
      }
    }
  });
  Ok(Server {
    task,
    _socket: socket,
    _lease: lease,
  })
}

async fn connection(
  mut stream: UnixStream,
  registry: Weak<Registry>,
  captures: Arc<Semaphore>,
) -> anyhow::Result<()> {
  ensure!(
    stream.peer_cred()?.uid() == path::uid(),
    "Native agent peer belongs to another user"
  );
  let request: Request = tokio::time::timeout(Duration::from_secs(5), wire::read(&mut stream))
    .await
    .context("Native agent request timed out")??;
  if matches!(request.operation, Operation::Control) {
    return super::control::serve(stream, registry, request.vm_id).await;
  }
  tokio::time::timeout(
    Duration::from_secs(165),
    single(stream, registry, request, captures),
  )
  .await
  .context("Native agent request timed out")?
}

async fn single(
  mut stream: UnixStream,
  registry: Weak<Registry>,
  request: Request,
  captures: Arc<Semaphore>,
) -> anyhow::Result<()> {
  let registry = registry
    .upgrade()
    .context("Hopper native VM registry is closed")?;
  let sessions = Sessions {
    inner: registry.clone(),
  };
  let mut capture_lease = None;
  let mut policy = None;
  let operation = async {
    let machine = registry.manager.machine(&request.vm_id, Actor::Agent)?;
    ensure!(
      machine.guest == model::GuestOs::Windows,
      "Native agent operation requires a Windows VM"
    );
    policy = Some(machine.agent_generation);
    match request.operation {
      Operation::Control => unreachable!("control was dispatched before registry retention"),
      Operation::Status => {
        let state = sessions
          .state(&request.vm_id, Actor::Agent)
          .await?
          .map(|state| match state {
            State::Running => Status::Running {},
            State::Paused => Status::Paused {},
            State::Stopped(reason) => Status::Stopped { reason },
            State::Failed(message) => Status::Failed { message },
          });
        Ok((Reply::State { state }, Vec::new()))
      }
      Operation::Capture => {
        capture_lease = Some(captures.acquire_owned().await?);
        let frame = sessions.capture(&request.vm_id, Actor::Agent).await?;
        ensure!(
          frame.rgba.len() == wire::frame_size(frame.width, frame.height)?,
          "Invalid native frame payload"
        );
        Ok((
          Reply::Frame {
            width: frame.width,
            height: frame.height,
            generation: frame.generation,
          },
          frame.rgba,
        ))
      }
    }
  };
  let mut trailing = [0; 1];
  let result = tokio::select! {
    result = operation => result,
    _ = stream.read(&mut trailing) => anyhow::bail!("Native agent disconnected or sent another request"),
  };
  let (reply, pixels) = match result {
    Ok(reply) => {
      ensure!(
        Some(
          registry
            .manager
            .machine(&request.vm_id, Actor::Agent)?
            .agent_generation
        ) == policy,
        "VM agent policy has changed"
      );
      reply
    }
    Err(error) => {
      let message: String = format!("{error:#}").chars().take(512).collect();
      (Reply::Error { message }, Vec::new())
    }
  };
  wire::write(&mut stream, &reply, &[]).await?;
  for chunk in pixels.chunks(64 * 1024) {
    ensure!(
      Some(
        registry
          .manager
          .machine(&request.vm_id, Actor::Agent)?
          .agent_generation
      ) == policy,
      "VM agent policy has changed"
    );
    stream.write_all(chunk).await?;
  }
  Ok(())
}
