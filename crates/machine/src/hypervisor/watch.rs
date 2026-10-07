use super::{check, ffi, Cpu};
use anyhow::{ensure, Context};

/// Request a native CPU exit at the deadline. The worker must return on cancellation.
pub fn bounded<T>(
  cpu: &mut Cpu<'_>,
  timeout: std::time::Duration,
  work: impl FnOnce(&mut Cpu<'_>) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
  watch(cpu, timeout, None, work)
}

/// Return regularly to the owner thread for input and display work. Handle Canceled exits.
pub fn paced<T>(
  cpu: &mut Cpu<'_>,
  timeout: std::time::Duration,
  interval: std::time::Duration,
  work: impl FnOnce(&mut Cpu<'_>) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
  ensure!(
    !interval.is_zero() && interval <= timeout,
    "Invalid CPU polling interval"
  );
  watch(cpu, timeout, Some(interval), work)
}

fn watch<T>(
  cpu: &mut Cpu<'_>,
  timeout: std::time::Duration,
  interval: Option<std::time::Duration>,
  work: impl FnOnce(&mut Cpu<'_>) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
  std::thread::scope(|scope| {
    let (done, receiver) = std::sync::mpsc::sync_channel(1);
    let mut id = cpu.id;
    let timer = scope.spawn(move || -> anyhow::Result<bool> {
      let start = std::time::Instant::now();
      loop {
        let remaining = timeout.saturating_sub(start.elapsed());
        let wait = interval.map_or(remaining, |interval| interval.min(remaining));
        if receiver.recv_timeout(wait) != Err(std::sync::mpsc::RecvTimeoutError::Timeout) {
          return Ok(false);
        }
        check(
          unsafe { ffi::hv_vcpus_exit(&mut id, 1) },
          "Interrupt CPU for owner thread",
        )?;
        if start.elapsed() >= timeout {
          return Ok(true);
        }
      }
    });
    let result = work(cpu);
    let _ = done.send(());
    let expired = timer
      .join()
      .map_err(|_| anyhow::anyhow!("CPU deadline worker panicked"))??;
    if expired {
      return result
        .and_then(|_| anyhow::bail!("Guest execution deadline expired"))
        .context("Guest execution deadline expired");
    }
    result
  })
}
