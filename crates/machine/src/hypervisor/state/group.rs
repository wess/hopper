//! one bounded owner-thread capture, bound to a runtime and coordinated pause.

use super::{clock, read, State};
use crate::{
  hypervisor::{pmu, Cpu},
  snapshot::native,
};

mod capture;
mod held;
mod recovery;
use anyhow::{ensure, Context};
pub(crate) use recovery::resume;
pub mod wire;
use std::{
  sync::{Arc, Mutex},
  time::{Duration, Instant},
};

pub struct Captured {
  pub cpu: State,
  pub clock: clock::Frozen,
  pub pmu: pmu::State,
}

pub struct Checkpoint {
  pub cpus: Vec<Captured>,
  pub primary: native::State,
}

struct Pending {
  sequence: u64,
  pause: u64,
  epoch: Option<u64>,
  started: Instant,
  slots: Vec<Option<Result<Captured, String>>>,
  primary: Option<Result<native::State, String>>,
}

#[derive(Default)]
struct Inner {
  bound: bool,
  closed: bool,
  pause: Option<u64>,
  sequence: u64,
  pending: Option<Pending>,
}

pub struct Group {
  count: usize,
  inner: Mutex<Inner>,
}

pub struct Capture {
  group: Arc<Group>,
  sequence: u64,
  done: bool,
}

pub(crate) struct Binding(Arc<Group>);

pub fn create(count: usize) -> anyhow::Result<Arc<Group>> {
  ensure!((1..=2).contains(&count), "Invalid CPU capture count");
  Ok(Arc::new(Group {
    count,
    inner: Mutex::new(Inner::default()),
  }))
}

impl Group {
  fn lock(&self) -> anyhow::Result<std::sync::MutexGuard<'_, Inner>> {
    self
      .inner
      .lock()
      .map_err(|_| anyhow::anyhow!("CPU capture lock poisoned"))
  }

  pub(crate) fn bind(self: &Arc<Self>, count: usize) -> anyhow::Result<Binding> {
    let mut inner = self.lock()?;
    ensure!(
      count == self.count && !inner.bound && !inner.closed,
      "CPU capture group cannot bind this runtime"
    );
    inner.bound = true;
    Ok(Binding(self.clone()))
  }

  pub(crate) fn enter(&self, pause: u64) -> anyhow::Result<()> {
    let mut inner = self.lock()?;
    ensure!(
      inner.bound && !inner.closed && inner.pause.is_none(),
      "CPU capture pause is unavailable"
    );
    inner.pause = Some(pause);
    Ok(())
  }

  pub(crate) fn leave(&self) {
    let mut inner = self.inner.lock().unwrap_or_else(|error| error.into_inner());
    inner.pause = None;
    inner.pending = None;
  }

  /// Request only from a paused runtime callback. Collection is armed after that
  /// callback and its interrupt updates finish; later callbacks wait for collection.
  pub fn request(self: &Arc<Self>) -> anyhow::Result<Capture> {
    let mut inner = self.lock()?;
    ensure!(
      inner.bound && !inner.closed,
      "CPU capture runtime is unavailable"
    );
    let pause = inner
      .pause
      .context("Pause every CPU before requesting capture")?;
    ensure!(inner.pending.is_none(), "CPU capture is already pending");
    let sequence = inner
      .sequence
      .checked_add(1)
      .context("CPU capture sequence exhausted")?;
    inner.sequence = sequence;
    inner.pending = Some(Pending {
      sequence,
      pause,
      epoch: None,
      started: Instant::now(),
      slots: (0..self.count).map(|_| None).collect(),
      primary: None,
    });
    Ok(Capture {
      group: self.clone(),
      sequence,
      done: false,
    })
  }

  pub(crate) fn collecting(&self) -> anyhow::Result<bool> {
    let inner = self.lock()?;
    Ok(inner.pending.as_ref().is_some_and(|pending| {
      pending.started.elapsed() < Duration::from_secs(5)
        && !matches!(&pending.primary, Some(Err(_)))
        && !pending
          .slots
          .iter()
          .any(|slot| matches!(slot, Some(Err(_))))
        && (pending.primary.is_none() || pending.slots.iter().any(Option::is_none))
    }))
  }

  pub(crate) fn arm(&self, pause: u64) -> anyhow::Result<()> {
    let mut inner = self.lock()?;
    if inner.pause == Some(pause) {
      if let Some(pending) = &mut inner.pending {
        if pending.pause == pause
          && pending.epoch.is_none()
          && matches!(&pending.primary, Some(Ok(_)))
        {
          pending.epoch = Some(clock::ticks());
        }
      }
    }
    Ok(())
  }

  pub(crate) fn record_primary(
    &self,
    pause: u64,
    capture: impl FnOnce() -> anyhow::Result<native::State>,
  ) -> anyhow::Result<()> {
    let sequence = {
      let inner = self.lock()?;
      let Some(pending) = &inner.pending else {
        return Ok(());
      };
      if inner.pause != Some(pause)
        || pending.pause != pause
        || pending.primary.is_some()
        || pending.started.elapsed() >= Duration::from_secs(5)
      {
        return Ok(());
      }
      pending.sequence
    };
    let captured =
      capture().map_err(|error| error.to_string().chars().take(512).collect::<String>());
    let mut inner = self.lock()?;
    if inner.pause == Some(pause) {
      if let Some(pending) = &mut inner.pending {
        if pending.sequence == sequence && pending.pause == pause && pending.primary.is_none() {
          pending.primary = Some(captured);
        }
      }
    }
    Ok(())
  }

  pub(crate) fn record(
    &self,
    cpu: &mut Cpu<'_>,
    index: usize,
    pause: u64,
    access: pmu::Token<'_>,
  ) -> anyhow::Result<()> {
    ensure!(index < self.count, "CPU capture index is invalid");
    let (sequence, epoch) = {
      let inner = self.lock()?;
      let Some(pending) = &inner.pending else {
        return Ok(());
      };
      if inner.pause != Some(pause)
        || pending.pause != pause
        || pending.slots[index].is_some()
        || pending.started.elapsed() >= Duration::from_secs(5)
      {
        return Ok(());
      }
      let Some(epoch) = pending.epoch else {
        return Ok(());
      };
      (pending.sequence, epoch)
    };
    let captured = (|| {
      ensure!(
        read(cpu, 0xc005)? & 0xff00ffffff == index as u64,
        "CPU capture affinity does not match its slot"
      );
      Ok((clock::capture_at(cpu, epoch)?, super::capture(cpu)?))
    })()
    .map_err(|error: anyhow::Error| error.to_string().chars().take(512).collect::<String>());
    let captured = match captured {
      Ok((clock, architectural)) => Ok(Captured {
        clock,
        cpu: architectural,
        // any context-cleanup failure makes this owner unusable; stop the runtime.
        pmu: access.capture(cpu)?,
      }),
      Err(error) => Err(error),
    };
    let mut inner = self.lock()?;
    if inner.pause == Some(pause) {
      if let Some(pending) = &mut inner.pending {
        if pending.sequence == sequence && pending.pause == pause && pending.slots[index].is_none()
        {
          pending.slots[index] = Some(captured);
        }
      }
    }
    Ok(())
  }
}

impl Drop for Binding {
  fn drop(&mut self) {
    let mut inner = self
      .0
      .inner
      .lock()
      .unwrap_or_else(|error| error.into_inner());
    inner.closed = true;
    inner.pause = None;
    inner.pending = None;
  }
}
