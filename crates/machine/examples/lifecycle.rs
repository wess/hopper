#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn main() -> anyhow::Result<()> {
  use anyhow::ensure;
  use machine::runtime::{self, Boot, Control, Reason};
  use std::time::{Duration, Instant};

  fn boot(code: &[u32], timeout: Duration) -> Boot {
    Boot {
      firmware: code.iter().flat_map(|word| word.to_le_bytes()).collect(),
      variables: vec![0xff; 0x40000],
      memory: 0x10000000,
      cpus: 2,
      timeout,
    }
  }
  let empty = || std::array::from_fn(|_| None);
  // mov w0, PSCI_SYSTEM_OFF / PSCI_SYSTEM_RESET; hvc #0.
  for (command, reason) in [(8, Reason::Shutdown), (9, Reason::Reset)] {
    let code = [0x52800000 | (command << 5), 0x72b08000, 0xd4000002];
    let stopped = runtime::run(boot(&code, Duration::from_secs(2)), empty(), |_, _, _| {
      Ok(Control::Continue)
    })?;
    ensure!(
      stopped.reason == reason,
      "Incorrect native guest power event"
    );
  }
  let idle = [0x14000000];
  let stopped = runtime::run(boot(&idle, Duration::from_secs(2)), empty(), |_, _, _| {
    Ok(Control::Stop)
  })?;
  ensure!(
    stopped.reason == Reason::Stopped,
    "Explicit stop was not retained"
  );
  drop(stopped);
  let error = runtime::run(boot(&idle, Duration::from_secs(2)), empty(), |_, _, _| {
    anyhow::bail!("lifecycle poll check")
  });
  ensure!(
    error
      .err()
      .is_some_and(|error| error.to_string() == "lifecycle poll check"),
    "Callback failure was not propagated"
  );
  let start = Instant::now();
  let error = runtime::run(
    boot(&idle, Duration::from_millis(100)),
    empty(),
    |_, _, _| Ok(Control::Continue),
  );
  let error = error
    .err()
    .ok_or_else(|| anyhow::anyhow!("Native timeout did not stop execution"))?;
  ensure!(
    format!("{error:#}").contains("timed out") || format!("{error:#}").contains("deadline expired"),
    "Native timeout returned an unrelated error"
  );
  ensure!(
    start.elapsed() < Duration::from_secs(5),
    "Native timeout failed to stop owner threads"
  );
  // reacquire the native VM after both failure paths in this same process.
  let stopped = runtime::run(boot(&idle, Duration::from_secs(2)), empty(), |_, _, _| {
    Ok(Control::Stop)
  })?;
  ensure!(
    stopped.reason == Reason::Stopped,
    "Native VM could not restart after teardown"
  );
  eprintln!("Native runtime lifecycle verified: shutdown, reset, stop, callback failure, timeout and reacquisition");
  Ok(())
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("Native execution requires an Apple silicon Mac")
}
