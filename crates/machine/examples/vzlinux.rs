#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn main() -> anyhow::Result<()> {
  use anyhow::{ensure, Context};
  use machine::vz::{self, Action, Boot, Linux};
  use objc2::MainThreadMarker;
  use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSRunLoop};
  use objc2_virtualization::VZVirtualMachineState as State;
  use std::{
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    time::{Duration, Instant},
  };

  let args: Vec<_> = std::env::args_os().skip(1).collect();
  ensure!(
    args.len() == 2,
    "Provide verified ARM64 kernel and initramfs paths"
  );
  let main = MainThreadMarker::new().context("VZ probe requires the main thread")?;
  let root = tempfile::tempdir()?;
  std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))?;
  let disk = root.path().join("disk");
  std::fs::OpenOptions::new()
    .create_new(true)
    .write(true)
    .mode(0o600)
    .open(&disk)?
    .set_len(1 << 30)?;
  let output = root.path().join("console");
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
    identity: vz::identity(),
    boot: Boot::Kernel {
      kernel: args[0].clone().into(),
      initramfs: Some(args[1].clone().into()),
      command_line: "console=hvc0 rdinit=/bin/sh".into(),
    },
    disk,
    installer: None,
    seed: None,
    network: None,
    shares: Vec::new(),
    console: Some(console),
  };
  let vm = vz::create(main, &boot)?;
  let run_loop = NSRunLoop::currentRunLoop();
  for (action, state) in [
    (Action::Start, State::Running),
    (Action::Pause, State::Paused),
    (Action::Resume, State::Running),
    (Action::Stop, State::Stopped),
  ] {
    let pending = vz::transition(&vm, action)?;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
      if let Some(result) = vz::poll(&pending)? {
        result?;
        break;
      }
      ensure!(Instant::now() < deadline, "VZ probe transition timed out");
      run_loop.runMode_beforeDate(
        unsafe { NSDefaultRunLoopMode },
        &NSDate::dateWithTimeIntervalSinceNow(0.01),
      );
    }
    ensure!(
      vz::state(&vm) == state,
      "VZ state differs from completed transition"
    );
    if matches!(action, Action::Start) {
      let deadline = Instant::now() + Duration::from_secs(30);
      loop {
        ensure!(
          std::fs::metadata(&output)?.len() <= 1024 * 1024,
          "VZ serial output exceeds bounds"
        );
        let log = std::fs::read(&output)?;
        let text = String::from_utf8_lossy(&log);
        if text.contains("Run /bin/sh as init process") && text.contains("can't access tty") {
          println!("Guest initramfs shell started through direct VZ");
          break;
        }
        if Instant::now() >= deadline {
          anyhow::bail!(
            "Linux boot timed out: {}",
            text.lines().rev().take(12).collect::<Vec<_>>().join("\n")
          );
        }
        run_loop.runMode_beforeDate(
          unsafe { NSDefaultRunLoopMode },
          &NSDate::dateWithTimeIntervalSinceNow(0.01),
        );
      }
    }
    println!("VZ state: {state:?}");
  }
  println!("Linux diagnostic boot verified; no desktop installation was performed");
  Ok(())
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("VZ probe requires Apple silicon macOS")
}
