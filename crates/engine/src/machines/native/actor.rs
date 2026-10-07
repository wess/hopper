use super::{wire, Pending, State};
use anyhow::{ensure, Context};
use model::native::{Command, Result as Reply};
use std::time::Duration;
use tokio::{
  process::{Child, ChildStdin},
  sync::{mpsc, watch},
};

pub(super) struct Reader(pub tokio::task::AbortHandle);

impl Drop for Reader {
  fn drop(&mut self) {
    self.0.abort();
  }
}

pub(super) async fn exchange(
  input: &mut ChildStdin,
  replies: &mut mpsc::Receiver<anyhow::Result<wire::Packet>>,
  id: u64,
  command: Command,
  limit: Duration,
) -> anyhow::Result<wire::Packet> {
  tokio::time::timeout(limit, async {
    wire::write(input, id, &command).await?;
    let packet = replies
      .recv()
      .await
      .context("Native worker disconnected")??;
    if packet.response.id == 0 && matches!(packet.response.result, Reply::Stopped { .. }) {
      return Ok(packet);
    }
    wire::validate(id, &command, &packet.response)?;
    Ok(packet)
  })
  .await
  .context("Native worker reply timed out")?
}

pub(super) async fn serve(
  mut child: Child,
  mut input: ChildStdin,
  mut replies: mpsc::Receiver<anyhow::Result<wire::Packet>>,
  mut queue: mpsc::Receiver<Pending>,
  state: watch::Sender<State>,
  _reader: Reader,
) {
  let result = run(&mut child, &mut input, &mut replies, &mut queue, &state).await;
  if let Err(error) = result {
    state.send_replace(State::Failed(error.to_string()));
  } else if matches!(
    tokio::time::timeout(Duration::from_secs(5), child.wait()).await,
    Ok(Ok(_))
  ) {
    return;
  }
  if child.try_wait().ok().flatten().is_none() {
    let _ = child.kill().await;
  }
  let _ = child.wait().await;
}

async fn run(
  child: &mut Child,
  input: &mut ChildStdin,
  replies: &mut mpsc::Receiver<anyhow::Result<wire::Packet>>,
  queue: &mut mpsc::Receiver<Pending>,
  state: &watch::Sender<State>,
) -> anyhow::Result<()> {
  let mut id = 1u64;
  loop {
    let pending = tokio::select! {
      biased;
      packet = replies.recv() => {
        let packet = packet.context("Native worker disconnected")??;
        ensure!(packet.response.id == 0, "Unexpected unsolicited native reply");
        let Reply::Stopped { reason } = packet.response.result else {
          anyhow::bail!("Unexpected unsolicited native event");
        };
        state.send_replace(State::Stopped(reason));
        return Ok(());
      }
      pending = queue.recv() => pending,
      status = child.wait() => {
        let status = status?;
        // stdout may still contain the final power event when process exit wins the race.
        if let Ok(Some(Ok(packet))) = tokio::time::timeout(Duration::from_secs(1), replies.recv()).await {
          if packet.response.id == 0 {
            if let Reply::Stopped { reason } = packet.response.result {
              state.send_replace(State::Stopped(reason));
              return Ok(());
            }
          }
        }
        anyhow::bail!("Native worker exited without a power event ({status})");
      }
    };
    id = id
      .checked_add(1)
      .context("Native request identifiers exhausted")?;
    let Some(pending) = pending else {
      let packet = exchange(input, replies, id, Command::Stop {}, Duration::from_secs(5)).await?;
      let Reply::Stopped { reason } = packet.response.result else {
        anyhow::bail!("Native worker did not stop");
      };
      state.send_replace(State::Stopped(reason));
      return Ok(());
    };
    if pending.response.is_closed() {
      continue;
    }
    let limit = if matches!(pending.command, Command::Capture {}) {
      120
    } else {
      30
    };
    let result = exchange(
      input,
      replies,
      id,
      pending.command,
      Duration::from_secs(limit),
    )
    .await;
    match result {
      Ok(packet) => {
        match packet.response.result {
          Reply::Paused {} | Reply::Status { paused: true, .. } => {
            state.send_replace(State::Paused);
          }
          Reply::Running {} | Reply::Status { paused: false, .. } => {
            state.send_replace(State::Running);
          }
          Reply::Stopped { reason } => {
            state.send_replace(State::Stopped(reason));
            let _ = pending.response.send(Ok(packet));
            return Ok(());
          }
          _ => {}
        }
        let _ = pending.response.send(Ok(packet));
      }
      Err(error) => {
        let message = error.to_string();
        let _ = pending.response.send(Err(error));
        anyhow::bail!(message);
      }
    }
  }
}
