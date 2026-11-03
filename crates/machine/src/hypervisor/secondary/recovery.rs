//! two owner acknowledgements: architectural restore, then a shared resume epoch.

use super::super::{state, Cpu};
use anyhow::{ensure, Context};
use std::{
  sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, RecvTimeoutError, SyncSender},
  },
  time::{Duration, Instant},
};

pub struct Recovery<'a> {
  saved: &'a state::group::Captured,
  epochs: Receiver<u64>,
  ready: SyncSender<()>,
}

pub struct Controller {
  epochs: SyncSender<u64>,
  ready: Receiver<()>,
}

pub fn create(saved: &state::group::Captured) -> (Recovery<'_>, Controller) {
  let (send, epochs) = mpsc::sync_channel(1);
  let (ready, receive) = mpsc::sync_channel(1);
  (
    Recovery {
      saved,
      epochs,
      ready,
    },
    Controller {
      epochs: send,
      ready: receive,
    },
  )
}

impl Controller {
  pub fn advance(&self, epoch: u64) -> anyhow::Result<()> {
    self
      .epochs
      .try_send(epoch)
      .context("Dispatch CPU recovery phase")?;
    self
      .ready
      .recv_timeout(Duration::from_secs(5))
      .context("Acknowledge CPU recovery phase")
  }
}

impl Recovery<'_> {
  pub(super) fn prepare(
    &self,
    cpu: &mut Cpu<'_>,
    affinity: u64,
    stop: &AtomicBool,
    access: super::super::pmu::Token<'_>,
  ) -> anyhow::Result<bool> {
    let Some(epoch) = self.epoch(Some(Duration::from_secs(5)), stop)? else {
      return Ok(false);
    };
    state::restore_frozen(cpu, &self.saved.cpu, &self.saved.clock, epoch)?;
    ensure!(
      state::read(cpu, 0xc005)? & 0xff00ffffff == affinity,
      "Restored CPU affinity changed"
    );
    self.ready.send(()).context("Publish restored CPU")?;
    let Some(epoch) = self.epoch(None, stop)? else {
      return Ok(false);
    };
    state::group::resume(cpu, self.saved, access, epoch)?;
    self.ready.send(()).context("Publish CPU resume epoch")?;
    Ok(true)
  }

  fn epoch(&self, timeout: Option<Duration>, stop: &AtomicBool) -> anyhow::Result<Option<u64>> {
    let started = Instant::now();
    loop {
      if stop.load(Ordering::Acquire) {
        return Ok(None);
      }
      ensure!(
        timeout.is_none_or(|limit| started.elapsed() < limit),
        "CPU recovery phase timed out"
      );
      match self.epochs.recv_timeout(Duration::from_millis(20)) {
        Ok(epoch) => return Ok(Some(epoch)),
        Err(RecvTimeoutError::Disconnected) => return Ok(None),
        Err(RecvTimeoutError::Timeout) => {}
      }
    }
  }
}
