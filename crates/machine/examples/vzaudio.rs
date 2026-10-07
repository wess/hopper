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
    args.len() == 3,
    "Provide diagnostic ARM64 kernel, audio initramfs and verified Alpine modloop"
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
  for (speakers, option, marker) in [
    (true, "enabled", "HOPPER_AUDIO_ENABLED_OK"),
    (false, "disabled", "HOPPER_AUDIO_DISABLED_OK"),
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
        command_line: format!("console=hvc0 rdinit=/init hopper.audio={option}"),
      },
      disk: disk.clone(),
      installer: None,
      seed: args.get(2).map(|path| path.clone().into()),
      network: Some(Mode::Disconnected),
      shares: Vec::new(),
      speakers,
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
        if text.contains(marker) {
          return Ok(());
        }
        if text.contains("HOPPER_AUDIO_FAILED") || Instant::now() >= deadline {
          anyhow::bail!(
            "Guest audio probe failed: {}",
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
    println!("Guest audio device verified: {option}");
  }
  println!("Output-only speaker device and disabled audio verified; audible playback and installed desktops remain unverified");
  Ok(())
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("VZ probe requires Apple silicon macOS")
}
