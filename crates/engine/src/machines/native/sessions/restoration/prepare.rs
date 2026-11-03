use super::{capture, Job, Ready};
use crate::machines::native::{self, restore::Owner, sessions::owner, State};
use anyhow::{ensure, Context};
use model::native::{Command, Result as Reply};
use std::{
  sync::{atomic::Ordering, Arc},
  time::{Duration, Instant},
};

impl Job {
  pub(super) async fn prepare(mut self) -> anyhow::Result<Ready> {
    let hardware = self
      .client
      .hardware
      .as_ref()
      .context("Missing restore hardware")?
      .clone();
    self.verify_components().await?;
    let running = match capture::prior(&self).await {
      Ok(running) => running,
      Err(error) => {
        if self.uncertain.load(Ordering::Acquire) {
          std::mem::forget((self.operation, self.session));
        }
        return Err(error);
      }
    };
    if let Err(error) = self.verify_checkpoint().await {
      capture::resume(&self, running).await;
      return Err(
        error.context("Restore source validation failed before stop; prior checkpoint retained"),
      );
    }
    if let Err(error) = (self.check)() {
      capture::resume(&self, running).await;
      return Err(error);
    }
    let stopped = native::transact(
      &self.client,
      Command::Stop {},
      Some(self.stop_check.clone()),
    )
    .await;
    if !matches!(&stopped, Ok(packet) if matches!(packet.response.result, Reply::Stopped { .. }))
      && matches!(native::state(&self.client), State::Running | State::Paused)
    {
      capture::resume(&self, running).await;
      stopped?;
      anyhow::bail!("Restore source did not acknowledge stop");
    }
    let reaped = owner::finished(self.session.as_ref().context("Missing restore session")?).await;
    if let Err(error) = reaped {
      std::mem::forget((self.operation, self.session));
      return Err(error.context("Restore source cleanup is unverified; operation remains owned"));
    }
    ensure!(
      matches!(stopped?.response.result, Reply::Stopped { .. }),
      "Restore source did not acknowledge stop"
    );
    (self.stop_check)()?;
    owner::retire(&self.slot, native::state(&self.client));
    self.session.take();
    let epoch = *self
      .slot
      .generation
      .lock()
      .map_err(|_| anyhow::anyhow!("VM generation lock failed"))?;
    let generation = self.slot.generation.clone();
    let cancel = self.cancel.clone();
    let policy = self.policy.clone();
    let deadline = Instant::now() + Duration::from_secs(300);
    let continuation: native::Check = Arc::new(move || {
      ensure!(
        !cancel.load(Ordering::Acquire),
        "Restore preparation caller cancelled"
      );
      ensure!(
        Instant::now() < deadline,
        "Restore preparation recovery deadline expired"
      );
      policy.check()?;
      ensure!(
        *generation
          .lock()
          .map_err(|_| anyhow::anyhow!("VM generation lock failed"))?
          == epoch,
        "VM generation changed before preparation recovery"
      );
      Ok(())
    });
    let owned = Owner::with_operation(
      self.manager.clone(),
      &self.id,
      self.policy.clone(),
      self.operation,
      hardware.namespace_stamp()?,
    )?;
    let preparation = super::preparation::run(
      owned,
      native::restore::Recovery {
        boot: hardware.boot,
        target: self.source.clone(),
        prior: self.rollback.clone(),
        predecessor: self.predecessor,
        configuration: self.client.configuration,
        check: continuation,
      },
    );
    let (prepared, failure) = tokio::select! {
      result = preparation => result?,
      _ = async {
        while !self.cancel.load(Ordering::Acquire) { tokio::time::sleep(Duration::from_millis(50)).await; }
      } => anyhow::bail!("Restore preparation caller cancelled"),
    };
    drop(self.session);
    Ok(Ready {
      prepared,
      helper: hardware.helper,
      rollback: self.rollback,
      failure,
      paused: !running,
    })
  }
}
