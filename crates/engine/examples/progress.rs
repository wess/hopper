#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn main() -> anyhow::Result<()> {
  use anyhow::{ensure, Context};
  use engine::machines::linux::{
    observe,
    progress::{self, Phase},
  };
  use machine::vz::{self, Action, Boot, Linux, MainThreadMarker};
  use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSRunLoop};
  use std::{
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    time::{Duration, Instant},
  };

  let args: Vec<_> = std::env::args_os().skip(1).collect();
  ensure!(
    args.len() == 2,
    "Provide diagnostic ARM64 kernel and progress initramfs"
  );
  let main = MainThreadMarker::new().context("Progress probe requires the main thread")?;
  let root = tempfile::tempdir()?;
  std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))?;
  let disk = root.path().join("disk");
  std::fs::OpenOptions::new()
    .create_new(true)
    .write(true)
    .mode(0o600)
    .open(&disk)?
    .set_len(1 << 30)?;
  let (output, observation) = observe::start(root.path(), "8197e0f0-0603-43e9-a817-eaf7ab0327af")?;
  let boot = Linux {
    cpus: 2,
    memory: 512 << 20,
    width: 1024,
    height: 768,
    identity: vz::identity(),
    boot: Boot::Kernel {
      kernel: args[0].clone().into(),
      initramfs: Some(args[1].clone().into()),
      command_line: "console=hvc0 rdinit=/init".into(),
    },
    disk,
    installer: None,
    seed: None,
    network: None,
    shares: Vec::new(),
    console: Some(output),
  };
  let mut vm = vz::create(main, &boot)?;
  vz::retain(&mut vm, std::sync::Arc::new(observation))?;
  let run_loop = NSRunLoop::currentRunLoop();
  let wait = |pending: vz::Pending| -> anyhow::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
      if let Some(result) = vz::poll(&pending)? {
        return result;
      }
      ensure!(Instant::now() < deadline, "VZ transition timed out");
      run_loop.runMode_beforeDate(
        unsafe { NSDefaultRunLoopMode },
        &NSDate::dateWithTimeIntervalSinceNow(0.01),
      );
    }
  };
  wait(vz::transition(&vm, Action::Start)?)?;
  let result = (|| -> anyhow::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
      if progress::read(root.path())? == Some(Phase::Deployed) {
        return Ok(());
      }
      ensure!(Instant::now() < deadline, "Guest progress was not observed");
      run_loop.runMode_beforeDate(
        unsafe { NSDefaultRunLoopMode },
        &NSDate::dateWithTimeIntervalSinceNow(0.01),
      );
    }
  })();
  let stopped = wait(vz::transition(&vm, Action::Stop)?);
  result?;
  stopped?;
  ensure!(
    std::fs::metadata(root.path().join("installation"))?.len() < 256,
    "Serial noise reached the journal"
  );
  println!("Native guest serial pipe drained 1 MiB of noise and persisted scoped phases; no Ubuntu installation or desktop readiness was verified");
  Ok(())
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("VZ probe requires Apple silicon macOS")
}
