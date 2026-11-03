//! bounded private-parent staging lease; only the host's policy guard can authorize commit.

use anyhow::ensure;
use std::{
  sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
  },
  time::{Duration, Instant},
};

pub(super) struct Grant {
  cancelled: AtomicBool,
  renewal: Mutex<Instant>,
  deadline: Instant,
}

impl Grant {
  pub fn new() -> Self {
    Self {
      cancelled: AtomicBool::new(false),
      renewal: Mutex::new(Instant::now()),
      deadline: Instant::now() + Duration::from_secs(300),
    }
  }

  pub fn check(&self) -> anyhow::Result<()> {
    let valid = !self.cancelled.load(Ordering::Acquire)
      && Instant::now() < self.deadline
      && self
        .renewal
        .lock()
        .map(|renewed| renewed.elapsed() < Duration::from_secs(5))
        .unwrap_or(false);
    if !valid {
      self.cancel();
    }
    ensure!(valid, "Private snapshot staging lease expired");
    Ok(())
  }

  pub fn renew(&self) -> anyhow::Result<()> {
    self.check()?;
    *self
      .renewal
      .lock()
      .map_err(|_| anyhow::anyhow!("Snapshot renewal lock failed"))? = Instant::now();
    self.check()
  }

  pub fn cancel(&self) {
    self.cancelled.store(true, Ordering::Release);
  }
}
