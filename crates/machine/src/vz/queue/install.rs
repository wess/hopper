use super::{validate, Check, Client, Owner, Request};
use crate::vz::install;
use anyhow::{ensure, Context};
use std::{
  path::PathBuf,
  sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
  },
  time::{Duration, Instant},
};
use tokio::sync::{oneshot, watch};

pub struct Installation {
  receive: Option<oneshot::Receiver<anyhow::Result<()>>>,
  cancelled: Arc<AtomicBool>,
  progress: watch::Receiver<f64>,
  check: Check,
}

pub(super) struct RequestInstall {
  id: String,
  path: PathBuf,
  check: Check,
  send: oneshot::Sender<anyhow::Result<()>>,
  progress: watch::Sender<f64>,
}

pub(super) struct Operation {
  pending: install::Pending,
  check: Check,
  send: oneshot::Sender<anyhow::Result<()>>,
  progress: watch::Sender<f64>,
  error: Option<anyhow::Error>,
}

impl Installation {
  pub fn cancel(&self) {
    self.cancelled.store(true, Ordering::Release);
  }

  pub fn fraction(&self) -> f64 {
    *self.progress.borrow()
  }

  pub async fn changed(&mut self) -> anyhow::Result<f64> {
    (self.check)()?;
    self
      .progress
      .changed()
      .await
      .context("macOS installation progress ended")?;
    (self.check)()?;
    Ok(self.fraction())
  }

  pub async fn wait(mut self) -> anyhow::Result<()> {
    let result = self
      .receive
      .take()
      .context("macOS installation result already consumed")?
      .await
      .context("macOS installation ended without a result")?;
    (self.check)()?;
    result
  }
}

impl Drop for Installation {
  fn drop(&mut self) {
    self.cancel();
  }
}

fn request(id: &str, path: PathBuf, original: Check) -> (RequestInstall, Installation) {
  let (send, receive) = oneshot::channel();
  let (progress, updates) = watch::channel(0.0);
  let cancelled = Arc::new(AtomicBool::new(false));
  let requested = cancelled.clone();
  let deadline = Instant::now() + Duration::from_secs(12 * 60 * 60);
  let check: Check = Arc::new(move || {
    original()?;
    ensure!(
      !requested.load(Ordering::Acquire),
      "macOS installation caller cancelled"
    );
    ensure!(Instant::now() < deadline, "macOS installation timed out");
    Ok(())
  });
  (
    RequestInstall {
      id: id.into(),
      path,
      check: check.clone(),
      send,
      progress,
    },
    Installation {
      receive: Some(receive),
      progress: updates,
      check,
      cancelled,
    },
  )
}

impl Client {
  pub async fn install_checked(
    &self,
    id: &str,
    path: PathBuf,
    check: Check,
  ) -> anyhow::Result<Installation> {
    validate(id)?;
    check()?;
    let (request, installation) = request(id, path, check);
    self
      .0
      .send(Request::Install(request))
      .await
      .context("VZ owner is unavailable")?;
    self.1.notify_one();
    Ok(installation)
  }
}

impl Owner {
  pub fn install_checked(
    &mut self,
    id: &str,
    path: PathBuf,
    check: Check,
  ) -> anyhow::Result<Installation> {
    validate(id)?;
    check()?;
    let (request, installation) = request(id, path, check);
    self.install(request);
    Ok(installation)
  }

  pub(super) fn install(&mut self, request: RequestInstall) {
    if request.send.is_closed() {
      return;
    }
    let result = (|| {
      (request.check)()?;
      ensure!(
        !self.pending.contains_key(&request.id) && !self.installations.contains_key(&request.id),
        "Another VZ operation is active"
      );
      let vm = self
        .machines
        .get(&request.id)
        .context("VZ machine is not owned")?;
      ensure!(!vm.stop_requested.get(), "macOS installation was stopped");
      install::start_checked(vm, &request.path, Some(request.check.clone()))
    })();
    match result {
      Ok(pending) => {
        self.installations.insert(
          request.id,
          Operation {
            pending,
            check: request.check,
            send: request.send,
            progress: request.progress,
            error: None,
          },
        );
      }
      Err(error) => {
        let _ = request.send.send(Err(error));
      }
    }
  }

  pub(super) fn tick_installations(&mut self) {
    let mut finished = Vec::new();
    for (id, operation) in &mut self.installations {
      if operation.error.is_none() {
        let result = (|| {
          (operation.check)()?;
          ensure!(
            !operation.send.is_closed(),
            "macOS installation caller cancelled"
          );
          ensure!(
            !self
              .machines
              .get(id)
              .context("macOS hardware lost ownership")?
              .stop_requested
              .get(),
            "macOS installation was stopped"
          );
          Ok(())
        })();
        if let Err(error) = result {
          operation.error = Some(error);
          install::cancel_pending(&operation.pending);
        }
      }
      operation
        .progress
        .send_replace(install::fraction_pending(&operation.pending));
      match install::poll_pending(&operation.pending) {
        Ok(None) => {}
        Ok(Some(result)) => finished.push((id.clone(), result)),
        Err(error) => finished.push((id.clone(), Err(error))),
      }
    }
    for (id, result) in finished {
      if let Some(operation) = self.installations.remove(&id) {
        let result = operation.error.map_or(result, Err);
        let _ = operation.send.send(result);
      }
    }
  }
}
