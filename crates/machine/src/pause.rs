use anyhow::{ensure, Context};
use std::{
  sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Condvar, Mutex,
  },
  time::{Duration, Instant},
};

#[derive(Default)]
struct State {
  generation: u64,
  requested: Option<u64>,
  acknowledged: Option<u64>,
}

#[derive(Default)]
pub struct Pause {
  state: Mutex<State>,
  changed: Condvar,
}

pub fn create() -> Arc<Pause> {
  Arc::new(Pause::default())
}

pub fn request(pause: &Pause) -> anyhow::Result<u64> {
  let mut state = pause
    .state
    .lock()
    .map_err(|_| anyhow::anyhow!("CPU pause lock poisoned"))?;
  if let Some(generation) = state.requested {
    return Ok(generation);
  }
  state.generation = state
    .generation
    .checked_add(1)
    .context("CPU pause generation exhausted")?;
  state.requested = Some(state.generation);
  pause.changed.notify_all();
  Ok(state.generation)
}

pub fn wait(pause: &Pause, generation: u64, timeout: Duration) -> anyhow::Result<()> {
  let mut state = pause
    .state
    .lock()
    .map_err(|_| anyhow::anyhow!("CPU pause lock poisoned"))?;
  let started = Instant::now();
  loop {
    ensure!(
      state.requested == Some(generation),
      "CPU pause request was superseded"
    );
    if state.acknowledged == Some(generation) {
      return Ok(());
    }
    let remaining = timeout.saturating_sub(started.elapsed());
    ensure!(!remaining.is_zero(), "CPU pause acknowledgement timed out");
    state = pause
      .changed
      .wait_timeout(state, remaining)
      .map_err(|_| anyhow::anyhow!("CPU pause lock poisoned"))?
      .0;
  }
}

pub fn resume(pause: &Pause) {
  let mut state = pause
    .state
    .lock()
    .unwrap_or_else(|error| error.into_inner());
  state.requested = None;
  pause.changed.notify_all();
}

/// Pump bounded owner work until the other CPU reaches a quiescent checkpoint.
/// A returned value interrupts waiting (for example, guest shutdown).
pub fn wait_with<T>(
  pause: &Pause,
  generation: u64,
  timeout: Duration,
  mut progress: impl FnMut() -> anyhow::Result<Option<T>>,
) -> anyhow::Result<Option<T>> {
  let started = Instant::now();
  loop {
    let state = pause
      .state
      .lock()
      .map_err(|_| anyhow::anyhow!("CPU pause lock poisoned"))?;
    ensure!(
      state.requested == Some(generation),
      "CPU pause request was superseded"
    );
    if state.acknowledged == Some(generation) {
      return Ok(None);
    }
    ensure!(
      started.elapsed() < timeout,
      "CPU pause acknowledgement timed out"
    );
    drop(state);
    if let Some(value) = progress()? {
      return Ok(Some(value));
    }
    let state = pause
      .state
      .lock()
      .map_err(|_| anyhow::anyhow!("CPU pause lock poisoned"))?;
    ensure!(
      state.requested == Some(generation),
      "CPU pause request was superseded"
    );
    if state.acknowledged == Some(generation) {
      return Ok(None);
    }
    let remaining = timeout.saturating_sub(started.elapsed());
    ensure!(!remaining.is_zero(), "CPU pause acknowledgement timed out");
    drop(
      pause
        .changed
        .wait_timeout(state, remaining.min(Duration::from_millis(1)))
        .map_err(|_| anyhow::anyhow!("CPU pause lock poisoned"))?,
    );
  }
}

/// call only after native guest execution has returned to its CPU owner.
pub fn checkpoint(pause: &Pause, stop: &AtomicBool) -> anyhow::Result<bool> {
  checkpoint_with(pause, stop, |_| Ok(()))
}

/// Owner-thread work while held. Resume/stop may cancel the generation during work;
/// observers must bind publication to the supplied generation themselves.
pub fn checkpoint_with(
  pause: &Pause,
  stop: &AtomicBool,
  mut observe: impl FnMut(u64) -> anyhow::Result<()>,
) -> anyhow::Result<bool> {
  let mut state = pause
    .state
    .lock()
    .map_err(|_| anyhow::anyhow!("CPU pause lock poisoned"))?;
  while let Some(generation) = state.requested {
    if stop.load(Ordering::Acquire) {
      return Ok(false);
    }
    drop(state);
    observe(generation)?;
    state = pause
      .state
      .lock()
      .map_err(|_| anyhow::anyhow!("CPU pause lock poisoned"))?;
    if state.requested != Some(generation) {
      continue;
    }
    if stop.load(Ordering::Acquire) {
      return Ok(false);
    }
    state.acknowledged = Some(generation);
    pause.changed.notify_all();
    state = pause
      .changed
      .wait_timeout(state, Duration::from_millis(20))
      .map_err(|_| anyhow::anyhow!("CPU pause lock poisoned"))?
      .0;
  }
  state.acknowledged = None;
  Ok(!stop.load(Ordering::Acquire))
}

pub struct Stopper {
  stop: Arc<AtomicBool>,
  pause: Arc<Pause>,
}

pub fn stopper(stop: Arc<AtomicBool>, pause: Arc<Pause>) -> Stopper {
  Stopper { stop, pause }
}

impl Drop for Stopper {
  fn drop(&mut self) {
    self.stop.store(true, Ordering::Release);
    resume(&self.pause);
  }
}
