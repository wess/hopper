//! An input lease keeps exclusive ownership through asynchronous key release.

mod request;
mod watch;

use super::{Registry, Sessions};
use crate::machines::Actor;
use anyhow::{ensure, Context};
use std::sync::{
  atomic::{AtomicBool, Ordering},
  Arc, Weak,
};
use tokio::{sync::watch as channel, time::Instant};

pub struct Input {
  registry: Weak<Registry>,
  id: String,
  actor: Actor,
  generation: Arc<std::sync::Mutex<u64>>,
  epoch: u64,
  active: Arc<AtomicBool>,
  release: channel::Sender<bool>,
  activity: channel::Sender<Instant>,
  ended: channel::Receiver<Option<Result<(), String>>>,
}

impl Drop for Input {
  fn drop(&mut self) {
    self.active.store(false, Ordering::Release);
    self.release.send_replace(true);
  }
}

impl Input {
  pub fn is_active(&self) -> bool {
    self.active.load(Ordering::Acquire)
  }

  pub async fn close(mut self) -> anyhow::Result<()> {
    self.active.store(false, Ordering::Release);
    self.release.send_replace(true);
    loop {
      if let Some(result) = self.ended.borrow_and_update().clone() {
        return result.map_err(anyhow::Error::msg);
      }
      self
        .ended
        .changed()
        .await
        .context("Guest input cleanup ended unexpectedly")?;
    }
  }
}

impl Sessions {
  pub async fn acquire_input(&self, id: &str, actor: Actor) -> anyhow::Result<Input> {
    let slot = self.slot(id).await?;
    self.inner.manager.machine(id, actor)?;
    let gate = slot
      .input
      .clone()
      .try_lock_owned()
      .map_err(|_| anyhow::anyhow!("VM input is controlled by another connection"))?;
    let session = slot.session.lock().await;
    self.inner.manager.machine(id, actor)?;
    let client = &session.as_ref().context("VM is not running")?.client;
    ensure!(
      matches!(super::super::state(client), super::super::State::Running),
      "Guest input requires a running VM"
    );
    let epoch = *slot
      .generation
      .lock()
      .unwrap_or_else(|error| error.into_inner());
    ensure!(
      *gate != Some(epoch),
      "Guest input release failed; restart the VM before reconnecting"
    );
    let active = Arc::new(AtomicBool::new(true));
    let (release, released) = channel::channel(false);
    let (activity, changed) = channel::channel(Instant::now());
    let (end, ended) = channel::channel(None);
    let lease = Input {
      registry: Arc::downgrade(&self.inner),
      id: id.into(),
      actor,
      generation: slot.generation.clone(),
      epoch,
      active,
      release,
      activity,
      ended,
    };
    watch::start(&lease, gate, released, changed, client.state.clone(), end);
    Ok(lease)
  }
}
