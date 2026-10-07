use super::{wire, Message, Reply};
use crate::machines::{
  native::{
    actor::Reader,
    sessions::{Registry, Sessions},
  },
  Actor,
};
use anyhow::{ensure, Context};
use model::native::Command;
use std::{sync::Weak, time::Duration};
use tokio::{net::UnixStream, sync::mpsc};

pub(in crate::machines::native::sessions::remote) async fn serve(
  mut stream: UnixStream,
  registry: Weak<Registry>,
  id: String,
) -> anyhow::Result<()> {
  let acquire = async {
    let registry = registry.upgrade().context("Native VM registry is closed")?;
    ensure!(
      registry.manager.machine(&id, Actor::Agent)?.guest == model::GuestOs::Windows,
      "Native input requires a Windows VM"
    );
    Sessions { inner: registry }
      .acquire_input(&id, Actor::Agent)
      .await
  };
  let mut trailing = [0; 1];
  use tokio::io::AsyncReadExt;
  let result = tokio::select! {
    result = acquire => result,
    _ = stream.read(&mut trailing) => return Ok(()),
  };
  let owner = match result {
    Ok(owner) => owner,
    Err(error) => {
      wire::write(&mut stream, &error_reply(&error), &[]).await?;
      return Ok(());
    }
  };
  wire::write(&mut stream, &Reply::Owned {}, &[]).await?;
  let (mut read, mut write) = stream.into_split();
  let (send, mut messages) = mpsc::channel(1);
  let reader = tokio::spawn(async move {
    loop {
      let message = tokio::time::timeout(Duration::from_secs(5), wire::read::<Message>(&mut read))
        .await
        .context("Native input connection is inactive")
        .and_then(|result| result);
      let ended = message.is_err();
      if send.send(message).await.is_err() || ended {
        break;
      }
    }
  });
  let _reader = Reader(reader.abort_handle());
  let mut ended = owner.completion();
  loop {
    let message = tokio::select! {
      biased;
      changed = ended.changed() => {
        let _ = changed;
        let result = owner.close().await;
        let reply = match result {
          Ok(()) => Reply::Error { message: "Guest input ownership has ended".into() },
          Err(error) => error_reply(&error),
        };
        wire::write(&mut write, &reply, &[]).await?;
        return Ok(());
      }
      message = messages.recv() => message,
    };
    let Some(Ok(message)) = message else {
      return owner.close().await;
    };
    match message {
      Message::Release {} => {
        let result = owner.close().await;
        let reply = match result {
          Ok(()) => Reply::Released {},
          Err(error) => error_reply(&error),
        };
        wire::write(&mut write, &reply, &[]).await?;
        return Ok(());
      }
      Message::Input { device, events } => {
        if events.is_empty() || events.len() > 64 {
          owner.close().await?;
          wire::write(
            &mut write,
            &Reply::Error {
              message: "Native remote input batches require 1–64 events".into(),
            },
            &[],
          )
          .await?;
          return Ok(());
        }
        let result = tokio::select! {
          result = owner.send(Command::Input { device, events }) => result,
          _ = messages.recv() => {
            // disconnect, malformed data or pipelining cancels the outstanding input operation.
            return owner.close().await;
          }
        };
        match result {
          Ok(_) => wire::write(&mut write, &Reply::Accepted {}, &[]).await?,
          Err(error) => {
            let _ = owner.close().await;
            wire::write(&mut write, &error_reply(&error), &[]).await?;
            return Ok(());
          }
        }
      }
    }
  }
}

fn error_reply(error: &anyhow::Error) -> Reply {
  Reply::Error {
    message: format!("{error:#}").chars().take(512).collect(),
  }
}
