//! owned background publication; caller retains its VM operation/runtime ownership.

use super::{
  checkpoint,
  manifest::Binding,
  package::{Package, Sealed},
};
use crate::hypervisor::state::group::Checkpoint;
use anyhow::{ensure, Context};
use std::{
  fs::File,
  sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
  },
  thread::JoinHandle,
  time::{Duration, Instant},
};

pub type Check = Arc<dyn Fn() -> anyhow::Result<()> + Send + Sync>;

pub struct Publication<O, T = Package> {
  worker: Option<JoinHandle<anyhow::Result<T>>>,
  cancelled: Arc<AtomicBool>,
  deadline: Instant,
  check: Check,
  _ownership: Arc<O>,
}

/// Move the immutable cut to its writer, then let the VM resume.
/// The supplied ownership token survives completion delivery and writer teardown.
/// `check` must retain the original policy and runtime generation, not just current access.
/// `acquire` returns a commit guard excluding policy changes until atomic publication completes.
/// Cancellation after atomic rename can leave a complete package; never replace it.
pub fn start<O: Send + Sync + 'static, G>(
  saved: Checkpoint,
  parent: File,
  binding: Binding,
  ownership: O,
  timeout: Duration,
  check: Check,
  acquire: impl FnOnce() -> anyhow::Result<G> + Send + 'static,
) -> anyhow::Result<Publication<O>> {
  spawn(ownership, timeout, check, move |authorize| {
    checkpoint::save(&saved, &parent, binding, authorize, acquire)
  })
}

/// Serialize an immutable cut in the background without granting commit authority.
/// The caller retains original policy/runtime/operation owners through the host handoff.
pub fn stage<O: Send + Sync + 'static>(
  saved: Checkpoint,
  parent: File,
  binding: Binding,
  ownership: O,
  timeout: Duration,
  check: Check,
) -> anyhow::Result<Publication<O, Sealed>> {
  spawn(ownership, timeout, check, move |authorize| {
    checkpoint::stage(&saved, &parent, binding, authorize)
  })
}

fn spawn<O: Send + Sync + 'static, T: Send + 'static>(
  ownership: O,
  timeout: Duration,
  check: Check,
  work: impl FnOnce(&dyn Fn() -> anyhow::Result<()>) -> anyhow::Result<T> + Send + 'static,
) -> anyhow::Result<Publication<O, T>> {
  ensure!(
    (Duration::from_millis(1)..=Duration::from_secs(3600)).contains(&timeout),
    "Invalid snapshot publication timeout"
  );
  let deadline = Instant::now()
    .checked_add(timeout)
    .context("Snapshot deadline overflow")?;
  let cancelled = Arc::new(AtomicBool::new(false));
  authorize(&check, &cancelled, deadline)?;
  let ownership = Arc::new(ownership);
  let owner = ownership.clone();
  let checks = check.clone();
  let cancel = cancelled.clone();
  let worker = std::thread::Builder::new()
    .name("hopper.snapshot".into())
    .spawn(move || {
      let check = || authorize(&checks, &cancel, deadline);
      let result = (|| {
        check()?;
        let result = work(&check);
        check()?;
        result
      })();
      drop(owner);
      result
    })
    .context("Start snapshot writer")?;
  Ok(Publication {
    worker: Some(worker),
    cancelled,
    deadline,
    check,
    _ownership: ownership,
  })
}

impl<O, T> Publication<O, T> {
  /// One completion only; original authorization also brackets delivery of cached results.
  pub fn poll(&mut self) -> anyhow::Result<Option<T>> {
    authorize(&self.check, &self.cancelled, self.deadline)?;
    let worker = self
      .worker
      .as_ref()
      .context("Snapshot publication already collected")?;
    if !worker.is_finished() {
      return Ok(None);
    }
    let result = self
      .worker
      .take()
      .context("Snapshot writer disappeared")?
      .join()
      .map_err(|_| anyhow::anyhow!("Snapshot writer panicked"));
    authorize(&self.check, &self.cancelled, self.deadline)?;
    result?.map(Some)
  }

  /// Retain cancellation/deadline checks while committing a collected stage under host policy.
  pub fn check(&self) -> anyhow::Result<()> {
    authorize(&self.check, &self.cancelled, self.deadline)
  }

  pub fn cancel(&self) {
    self.cancelled.store(true, Ordering::Release);
  }
}

impl<O, T> Drop for Publication<O, T> {
  fn drop(&mut self) {
    self.cancel();
    // filesystem calls can delay cancellation; never detach a writer with owned resources.
    if let Some(worker) = self.worker.take() {
      let _ = worker.join();
    }
  }
}

fn authorize(check: &Check, cancelled: &AtomicBool, deadline: Instant) -> anyhow::Result<()> {
  let result = (|| {
    ensure!(
      !cancelled.load(Ordering::Acquire),
      "Snapshot publication cancelled"
    );
    ensure!(
      Instant::now() < deadline,
      "Snapshot publication deadline expired"
    );
    check()?;
    ensure!(
      !cancelled.load(Ordering::Acquire),
      "Snapshot publication cancelled"
    );
    ensure!(
      Instant::now() < deadline,
      "Snapshot publication deadline expired"
    );
    Ok(())
  })();
  if result.is_err() {
    cancelled.store(true, Ordering::Release);
  }
  result
}

#[cfg(test)]
#[path = "../../tests/publication/owner.rs"]
mod tests;

#[cfg(test)]
#[path = "../../tests/publication/staging.rs"]
mod staging;
