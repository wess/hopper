use super::{Input, Sessions};
use anyhow::{ensure, Context};
use model::native::{Command, Result as Reply};
use std::sync::{atomic::Ordering, Arc};
use tokio::time::Instant;

struct Cancel {
  active: Arc<std::sync::atomic::AtomicBool>,
  release: Option<tokio::sync::watch::Sender<bool>>,
}

impl Drop for Cancel {
  fn drop(&mut self) {
    if let Some(release) = self.release.take() {
      self.active.store(false, Ordering::Release);
      release.send_replace(true);
    }
  }
}

impl Input {
  pub async fn send(&self, command: Command) -> anyhow::Result<Reply> {
    let Command::Input { events, .. } = &command else {
      anyhow::bail!("An input lease accepts guest input only");
    };
    ensure!(
      !events.is_empty() && events.len() <= 128,
      "Invalid native input batch"
    );
    ensure!(
      self.active.load(Ordering::Acquire),
      "Guest input ownership has ended"
    );
    let registry = self
      .registry
      .upgrade()
      .context("Native VM registry is closed")?;
    let sessions = Sessions { inner: registry };
    let active = self.active.clone();
    let generation = self.generation.clone();
    let epoch = self.epoch;
    let owner: super::super::super::Check = Arc::new(move || {
      ensure!(
        active.load(Ordering::Acquire),
        "Guest input ownership has ended"
      );
      ensure!(
        *generation.lock().unwrap_or_else(|error| error.into_inner()) == epoch,
        "Guest input belongs to a previous worker"
      );
      Ok(())
    });
    let mut cancel = Cancel {
      active: self.active.clone(),
      release: Some(self.release.clone()),
    };
    self.activity.send_replace(Instant::now());
    let result = sessions
      .request_owned(&self.id, self.actor, command, owner)
      .await
      .and_then(|reply| {
        ensure!(
          matches!(reply, Reply::Accepted {}),
          "Native worker did not accept input"
        );
        Ok(reply)
      });
    if result.is_ok() {
      self.activity.send_replace(Instant::now());
      cancel.release.take();
    }
    result
  }
}

impl Sessions {
  pub(super) async fn release_input(&self, id: &str, epoch: u64) -> anyhow::Result<()> {
    let slot = self.slot(id).await?;
    let session = slot.session.lock().await;
    if *slot
      .generation
      .lock()
      .unwrap_or_else(|error| error.into_inner())
      != epoch
    {
      return Ok(());
    }
    let Some(session) = session.as_ref() else {
      return Ok(());
    };
    if matches!(
      super::super::super::state(&session.client),
      super::super::super::State::Stopped(_) | super::super::super::State::Failed(_)
    ) {
      return Ok(());
    }
    let result = super::super::super::request(&session.client, Command::Release {}).await?;
    ensure!(
      matches!(result, Reply::Accepted {}),
      "Native worker did not release input"
    );
    Ok(())
  }
}
