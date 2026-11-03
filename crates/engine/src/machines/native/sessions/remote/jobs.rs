use super::{wire, Operation};
use crate::machines::{native::sessions::Registry, Actor, Machines};
use anyhow::{ensure, Context};
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use model::guest::wire::Reply;
use model::guest::wire::{Action, Decoder, Direction, Message, Outcome, Request, CHUNK};
use serde::{Deserialize, Serialize};
use std::{sync::Weak, time::Duration};
use tokio::{
  io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
  net::UnixStream,
};

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
enum Header {
  Guest { length: usize },
  Error { message: String },
}

pub async fn read_packet(
  stream: &mut (impl AsyncRead + Unpin),
  length: usize,
  direction: Direction,
) -> anyhow::Result<Message> {
  let limit = match direction {
    Direction::Request => 256 << 10,
    Direction::Reply => 1 << 20,
  };
  ensure!(
    (9..=limit + 8).contains(&length),
    "Guest packet length exceeds bounds"
  );
  let mut decoder = Decoder::new(direction, model::GuestOs::Linux);
  let mut remaining = length;
  let mut bytes = vec![0; CHUNK.min(length)];
  while remaining > 0 {
    let count = remaining.min(bytes.len());
    stream.read_exact(&mut bytes[..count]).await?;
    decoder.feed(&bytes[..count])?;
    remaining -= count;
  }
  decoder.take()?.context("Guest packet is incomplete")
}

pub async fn request(manager: &Machines, id: &str, action: Action) -> anyhow::Result<Outcome> {
  let policy = manager.machine(id, Actor::Agent)?.agent_generation;
  request_scoped(manager, id, policy, action).await
}

pub async fn request_scoped(
  manager: &Machines,
  id: &str,
  policy: u64,
  action: Action,
) -> anyhow::Result<Outcome> {
  let original = manager.machine(id, Actor::Agent)?;
  ensure!(
    original.agent_generation == policy,
    "Original guest agent policy changed"
  );
  ensure!(
    original.guest == model::GuestOs::Linux
      && original.runtime == Some(model::MachineRuntime::Virtualization),
    "Guest commands require native Linux"
  );
  let bytes = Request {
    sequence: 1,
    action,
  }
  .encode(model::GuestOs::Linux)?;
  let result = tokio::time::timeout(Duration::from_secs(205), async {
    let mut stream = super::connect(
      manager,
      id,
      Operation::Guest {
        length: bytes.len(),
      },
      original.agent_generation,
    )
    .await?;
    stream.write_all(&bytes).await?;
    let header: Header = wire::read(&mut stream).await?;
    let length = match header {
      Header::Guest { length } => length,
      Header::Error { message } => {
        ensure!(
          message.len() <= 512 && !message.contains('\0'),
          "Invalid guest service error"
        );
        anyhow::bail!("{message}");
      }
    };
    let Message::Reply(reply) = read_packet(&mut stream, length, Direction::Reply).await? else {
      anyhow::bail!("Unexpected guest reply");
    };
    ensure!(reply.sequence == 1, "Unexpected guest reply sequence");
    Ok::<_, anyhow::Error>(reply.outcome)
  })
  .await
  .context("Guest agent service timed out")??;
  let current = manager.machine(id, Actor::Agent)?;
  ensure!(
    current.guest == original.guest
      && current.runtime == original.runtime
      && current.agent_generation == original.agent_generation,
    "Guest agent policy changed"
  );
  Ok(result)
}

pub(super) async fn serve(
  mut stream: UnixStream,
  registry: Weak<Registry>,
  id: String,
  policy: Option<u64>,
  length: usize,
) -> anyhow::Result<()> {
  let registry = registry.upgrade().context("VM registry is closed")?;
  let operation = async {
    let machine = registry.manager.machine(&id, Actor::Agent)?;
    ensure!(
      policy == Some(machine.agent_generation),
      "Guest commands require the original agent generation"
    );
    ensure!(
      machine.guest == model::GuestOs::Linux
        && machine.runtime == Some(model::MachineRuntime::Virtualization),
      "Guest commands require native Linux"
    );
    let Message::Request(request) = tokio::time::timeout(
      Duration::from_secs(5),
      read_packet(&mut stream, length, Direction::Request),
    )
    .await
    .context("Guest body timed out")??
    else {
      anyhow::bail!("Unexpected guest request");
    };
    ensure!(request.sequence == 1, "Unexpected guest request sequence");
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
      let service = registry
        .virtual_agents
        .lock()
        .map_err(|_| anyhow::anyhow!("Virtual agent service lock failed"))?
        .clone()
        .context("Virtual agent owner is not connected")?;
      ensure!(
        registry
          .manager
          .machine(&id, Actor::Agent)?
          .agent_generation
          == machine.agent_generation,
        "Guest agent policy changed"
      );
      let mut trailing = [0; 1];
      let result = tokio::select! {
        result = service.guest_request_scoped(&id, machine.agent_generation, request.action) => result?,
        _ = stream.read(&mut trailing) => anyhow::bail!("Guest agent disconnected or pipelined another request"),
      };
      ensure!(
        registry
          .manager
          .machine(&id, Actor::Agent)?
          .agent_generation
          == machine.agent_generation,
        "Guest agent policy changed"
      );
      Ok::<_, anyhow::Error>(
        Reply {
          sequence: 1,
          outcome: result,
        }
        .encode()?,
      )
    }
    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    {
      let _ = request;
      anyhow::bail!("Virtual guest commands require Apple silicon macOS");
    }
  };
  let bytes = match operation.await {
    Ok(bytes) => bytes,
    Err(error) => {
      let mut message = format!("{error:#}");
      let mut end = message.len().min(512);
      while !message.is_char_boundary(end) {
        end -= 1;
      }
      message.truncate(end);
      return wire::write(&mut stream, &Header::Error { message }, &[]).await;
    }
  };
  ensure!(
    Some(
      registry
        .manager
        .machine(&id, Actor::Agent)?
        .agent_generation
    ) == policy,
    "Guest agent policy changed"
  );
  wire::write(
    &mut stream,
    &Header::Guest {
      length: bytes.len(),
    },
    &[],
  )
  .await?;
  for chunk in bytes.chunks(CHUNK) {
    ensure!(
      Some(
        registry
          .manager
          .machine(&id, Actor::Agent)?
          .agent_generation
      ) == policy,
      "Guest agent policy changed"
    );
    stream.write_all(chunk).await?;
  }
  Ok(())
}
