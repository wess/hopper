#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use super::snapshot;
use super::{actor, wire, Check, Client, State};
use anyhow::{bail, Context};
use model::native::{Boot, Command, Result as Reply};
use std::{
  any::Any,
  path::Path,
  process::Stdio,
  sync::{Arc, Mutex, OnceLock},
  time::Duration,
};
use tokio::{
  process,
  sync::{mpsc, watch},
};

pub(super) enum Kind {
  Normal,
  Restore(model::native::Restore),
  Generation(model::native::Restore),
}

pub(super) type Keep = Arc<dyn Any + Send + Sync>;
pub(super) struct Scope {
  pub check: Check,
  pub keep: Keep,
}

pub(super) fn quarantine(keep: Keep) {
  static OWNERS: OnceLock<Mutex<Vec<Keep>>> = OnceLock::new();
  match OWNERS.get_or_init(|| Mutex::new(Vec::new())).lock() {
    Ok(mut owners) => owners.push(keep),
    Err(_) => std::mem::forget(keep),
  }
}

pub(super) async fn launch(
  helper: &Path,
  boot: Boot,
  kind: Kind,
  scope: Option<Scope>,
) -> anyhow::Result<Client> {
  if let Some(scope) = &scope {
    (scope.check)()?;
  }
  model::native::machine_identity(&boot.machine_id).context("Invalid native VM identity")?;
  #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
  let hardware = snapshot::Hardware::capture(&boot, helper).ok();
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
  let initial = if !matches!(kind, Kind::Normal) {
    State::Paused
  } else {
    State::Running
  };
  let command = match kind {
    Kind::Restore(restore) => Command::StartRestored { boot, restore },
    Kind::Generation(restore) => Command::StartGeneration { boot, restore },
    Kind::Normal => Command::Start { boot },
  };
  let startup = async {
    let exchange = actor::exchange(
      &mut input,
      &mut replies,
      1,
      command,
      Duration::from_secs(30),
    );
    tokio::pin!(exchange);
    let packet = loop {
      tokio::select! {
        packet = &mut exchange => break packet?,
        _ = tokio::time::sleep(Duration::from_millis(100)) => {
          if let Some(scope) = &scope { (scope.check)()?; }
        }
      }
    };
    if let Some(scope) = &scope {
      (scope.check)()?;
    }
    match packet.response.result {
      Reply::Started { configuration, .. }
      | Reply::Restored { configuration, .. }
      | Reply::GenerationStarted { configuration, .. } => Ok(configuration),
      Reply::Rejected { message } => bail!("Native worker rejected startup: {message}"),
      _ => bail!("Native worker stopped during startup"),
    }
  }
  .await;
  let configuration = match startup {
    Ok(configuration) => configuration,
    Err(error) => {
      let _ = child.kill().await;
      if let Err(cleanup) = child.wait().await {
        if let Some(scope) = scope {
          quarantine(scope.keep);
        }
        return Err(error).context(format!(
          "Worker cleanup unconfirmed; ownership retained: {cleanup}"
        ));
      }
      return Err(error);
    }
  };
  let (requests, queue) = mpsc::channel(8);
  let (state, status) = watch::channel(initial);
  let (finished, ended) = watch::channel(false);
  let observed = ended.clone();
  tokio::spawn(async move {
    actor::serve(child, input, replies, queue, state, reader, finished).await;
    if !*observed.borrow() {
      if let Some(scope) = scope {
        quarantine(scope.keep);
      }
    }
  });
  Ok(Client {
    configuration,
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    hardware,
    requests,
    state: status,
    ended,
  })
}
