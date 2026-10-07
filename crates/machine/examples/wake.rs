#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn main() -> anyhow::Result<()> {
  use anyhow::ensure;
  use machine::hypervisor as hv;
  use std::time::Duration;

  let vm = hv::create()?;
  let mut memory = hv::memory(&vm, 0x40000000, 0x10000)?;
  hv::write(&mut memory, 0, &0x14000000u32.to_le_bytes())?;
  let mut cpu = hv::cpu(&vm)?;
  hv::enter(&mut cpu, 0x40000000)?;
  let wake = hv::wake(&cpu);
  ensure!(hv::request_exit(&wake)?, "Live CPU rejected wake");
  hv::bounded(&mut cpu, Duration::from_secs(2), |cpu| {
    ensure!(
      matches!(hv::run(cpu)?, hv::Exit::Canceled),
      "Queued wake did not cancel entry"
    );
    Ok(())
  })?;
  std::thread::scope(|scope| -> anyhow::Result<()> {
    let handle = wake.clone();
    let (started, ready) = std::sync::mpsc::sync_channel(1);
    let worker = scope.spawn(move || -> anyhow::Result<()> {
      ready.recv_timeout(Duration::from_secs(2))?;
      std::thread::sleep(Duration::from_millis(20));
      ensure!(
        hv::request_exit(&handle)?,
        "Live CPU rejected cross-thread wake"
      );
      Ok(())
    });
    hv::bounded(&mut cpu, Duration::from_secs(2), |cpu| {
      started.send(())?;
      ensure!(
        matches!(hv::run(cpu)?, hv::Exit::Canceled),
        "Owner did not receive wake"
      );
      Ok(())
    })?;
    worker
      .join()
      .map_err(|_| anyhow::anyhow!("Wake thread panicked"))??;
    Ok(())
  })?;
  drop(cpu);
  ensure!(
    !hv::request_exit(&wake)?,
    "Retained wake targeted a destroyed CPU"
  );
  hv::write(&mut memory, 0x100, &0xd4000002u32.to_le_bytes())?;
  let mut replacement = hv::cpu(&vm)?;
  hv::enter(&mut replacement, 0x40000100)?;
  ensure!(
    !hv::request_exit(&wake)?,
    "Stale handle became active for a new CPU"
  );
  hv::bounded(&mut replacement, Duration::from_secs(2), |cpu| {
    ensure!(
      matches!(hv::run(cpu)?, hv::Exit::Exception { .. }),
      "Stale handle canceled the replacement CPU"
    );
    Ok(())
  })?;
  println!("Queued and cross-thread native wakes passed; destroyed CPU handles are inactive");
  Ok(())
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("The native hypervisor requires Apple silicon macOS")
}
