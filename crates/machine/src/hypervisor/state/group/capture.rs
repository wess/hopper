use super::{Capture, Checkpoint};
use anyhow::{ensure, Context};
use std::time::Duration;

impl Capture {
  pub fn poll(&mut self) -> anyhow::Result<Option<Checkpoint>> {
    ensure!(!self.done, "CPU capture has already completed");
    let mut inner = self.group.lock()?;
    let pending = inner
      .pending
      .as_ref()
      .filter(|pending| pending.sequence == self.sequence)
      .context("CPU capture was cancelled or its runtime resumed")?;
    if pending.started.elapsed() >= Duration::from_secs(5) {
      inner.pending = None;
      self.done = true;
      anyhow::bail!("CPU capture timed out");
    }
    if pending
      .slots
      .iter()
      .any(|slot| matches!(slot, Some(Err(_))))
      || matches!(&pending.primary, Some(Err(_)))
    {
      let pending = inner.pending.take().context("CPU capture disappeared")?;
      self.done = true;
      if let Some(Err(error)) = pending.primary {
        anyhow::bail!("Primary state capture failed: {error}");
      }
      for slot in pending.slots.into_iter().flatten() {
        if let Err(error) = slot {
          anyhow::bail!("CPU capture failed: {error}");
        }
      }
    }
    if inner
      .pending
      .as_ref()
      .is_some_and(|pending| pending.slots.iter().all(Option::is_some) && pending.primary.is_some())
    {
      let pending = inner.pending.take().context("CPU capture disappeared")?;
      self.done = true;
      let cpus = pending
        .slots
        .into_iter()
        .map(|slot| {
          slot
            .context("CPU capture is incomplete")?
            .map_err(anyhow::Error::msg)
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
      let primary = pending
        .primary
        .context("Primary state capture is incomplete")?
        .map_err(anyhow::Error::msg)?;
      return Ok(Some(Checkpoint { cpus, primary }));
    }
    Ok(None)
  }
}

impl Drop for Capture {
  fn drop(&mut self) {
    if !self.done {
      let mut inner = self
        .group
        .inner
        .lock()
        .unwrap_or_else(|error| error.into_inner());
      if inner
        .pending
        .as_ref()
        .is_some_and(|pending| pending.sequence == self.sequence)
      {
        inner.pending = None;
      }
    }
  }
}
