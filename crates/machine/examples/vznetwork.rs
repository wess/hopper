#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn main() -> anyhow::Result<()> {
  use anyhow::{ensure, Context};
  use machine::vz::{self, network::Mode, Action, Boot, Linux};
  use objc2::MainThreadMarker;
  use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSRunLoop};
  use std::{
    os::unix::fs::OpenOptionsExt,
    time::{Duration, Instant},
  };

  let args: Vec<_> = std::env::args_os().skip(1).collect();
  ensure!(
    (2..=3).contains(&args.len()),
    "Provide diagnostic ARM64 kernel, network initramfs and optional NoCloud seed"
  );
  let main = MainThreadMarker::new().context("VZ probe requires the main thread")?;
  let root = tempfile::tempdir()?;
  let disk = root.path().join("disk");
  std::fs::OpenOptions::new()
    .create_new(true)
    .write(true)
    .mode(0o600)
    .open(&disk)?
    .set_len(1 << 30)?;
  let identity = vz::identity();
  let run_loop = NSRunLoop::currentRunLoop();
  for (mode, option, marker) in [
    (Mode::Nat, "online", "HOPPER_NETWORK_NAT_OK"),
    (Mode::Disconnected, "offline", "HOPPER_NETWORK_OFFLINE_OK"),
  ] {
    let output = root.path().join(option);
    let console = std::fs::OpenOptions::new()
      .create_new(true)
      .write(true)
      .mode(0o600)
      .open(&output)?;
    let boot = Linux {
      cpus: 2,
      memory: 512 << 20,
      width: 1024,
      height: 768,
      identity: identity.clone(),
      boot: Boot::Kernel {
        kernel: args[0].clone().into(),
        initramfs: Some(args[1].clone().into()),
        command_line: format!(
          "console=hvc0 rdinit=/init hopper.network={option} hopper.seed={}",
          if args.len() == 3 { 1 } else { 0 }
        ),
      },
      disk: disk.clone(),
      installer: None,
      seed: args.get(2).map(|path| path.clone().into()),
      network: Some(mode),
      console: Some(console),
    };
    let vm = vz::create(main, &boot)?;
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
        ensure!(
          std::fs::metadata(&output)?.len() <= 1 << 20,
          "Serial output exceeds bounds"
        );
        let text = std::fs::read_to_string(&output)?;
        if text.contains(marker) && (args.len() == 2 || text.contains("HOPPER_SEED_OK")) {
          return Ok(());
        }
        if text.contains("HOPPER_NETWORK_FAILED") || Instant::now() >= deadline {
          anyhow::bail!(
            "Guest network probe failed: {}",
            text.lines().rev().take(15).collect::<Vec<_>>().join("\n")
          );
        }
        run_loop.runMode_beforeDate(
          unsafe { NSDefaultRunLoopMode },
          &NSDate::dateWithTimeIntervalSinceNow(0.01),
        );
      }
    })();
    let stopped = wait(vz::transition(&vm, Action::Stop)?);
    result?;
    stopped?;
    if args.len() == 3 {
      println!("Read-only NoCloud seed files and account configuration verified in the guest");
    }
    println!("Guest network verified: {mode:?}");
  }
  println!("NAT DHCP, DNS and public download plus disconnected guest link verified; no OS installation was performed");
  Ok(())
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("VZ probe requires Apple silicon macOS")
}
