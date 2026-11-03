//! local runtime actions retain relative delays, never the old host's absolute deadline.

use anyhow::{ensure, Context};
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct State {
  pub(super) tail: Vec<u8>,
  pub(super) enter: Option<Duration>,
}

pub fn capture(tail: &[u8], enter: Option<Instant>, now: Instant) -> anyhow::Result<State> {
  ensure!(tail.len() <= 64, "UART diagnostic tail exceeds its bound");
  let enter = enter.map(|deadline| deadline.saturating_duration_since(now));
  ensure!(
    enter.is_none_or(|delay| delay <= Duration::from_secs(1)),
    "Pending boot action exceeds its delay bound"
  );
  Ok(State {
    tail: tail.to_vec(),
    enter,
  })
}

impl State {
  pub fn tail(&self) -> &[u8] {
    &self.tail
  }

  pub fn resume(&self, now: Instant) -> anyhow::Result<Option<Instant>> {
    self
      .enter
      .map(|delay| {
        now
          .checked_add(delay)
          .context("Pending boot action deadline overflow")
      })
      .transpose()
  }
}
