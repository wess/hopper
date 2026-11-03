#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn main() -> anyhow::Result<()> {
  use anyhow::{ensure, Context};
  use machine::vz::{self, Action, Boot, Linux};
  use objc2::MainThreadMarker;
  use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSRunLoop};
  use std::{
    os::unix::fs::OpenOptionsExt,
    time::{Duration, Instant},
  };

  let args: Vec<_> = std::env::args_os().skip(1).collect();
  ensure!(
    args.len() == 2,
    "Provide verified ARM64 kernel and guest test initramfs"
  );
  let main = MainThreadMarker::new().context("Guest job probe requires the main thread")?;
  let root = tempfile::Builder::new()
    .prefix("hopperguesttests90.")
    .tempdir_in("/tmp")?
    .keep();
  eprintln!("Linux harness evidence: {}", root.display());
  let disk = root.join("disk");
  std::fs::OpenOptions::new()
    .create_new(true)
    .write(true)
    .mode(0o600)
    .open(&disk)?
    .set_len(1 << 30)?;
  let output = root.join("console");
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
      command_line: "console=hvc0 rdinit=/init".into(),
    },
    disk,
    installer: None,
    seed: None,
    network: None,
    console: Some(console),
    shares: Vec::new(),
    speakers: false,
  };
  let vm = vz::create(main, &boot)?;
  let run_loop = NSRunLoop::currentRunLoop();
  let tick = || {
    run_loop.runMode_beforeDate(
      unsafe { NSDefaultRunLoopMode },
      &NSDate::dateWithTimeIntervalSinceNow(0.01),
    );
  };
  let wait = |pending: vz::Pending| -> anyhow::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
      if let Some(result) = vz::poll(&pending)? {
        return result;
      }
      ensure!(Instant::now() < deadline, "VZ transition timed out");
      tick();
    }
  };
  wait(vz::transition(&vm, Action::Start)?)?;
  let result = (|| -> anyhow::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
      ensure!(
        std::fs::metadata(&output)?.len() <= 1 << 20,
        "Serial output exceeds bounds"
      );
      let text = std::fs::read_to_string(&output)?;
      if text.contains("HOPPER_TESTS_OK") {
        ensure!(
          text.matches("test result: ok.").count() == 12
            && text.matches("HOPPER_TEST_HARNESS root ").count() == 6
            && text.matches("HOPPER_TEST_HARNESS normal ").count() == 6,
          "Incomplete Linux harness evidence"
        );
        return Ok(());
      }
      if text.contains("HOPPER_TESTS_FAILED") || Instant::now() >= deadline {
        anyhow::bail!(
          "Guest job diagnostic failed: {}",
          text.lines().rev().take(20).collect::<Vec<_>>().join("\n")
        );
      }
      tick();
    }
  })();
  let stopped = wait(vz::transition(&vm, Action::Stop)?);
  result?;
  stopped?;
  println!("Linux guest test harnesses passed as root and UID 1001");
  Ok(())
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("VZ guest job probe requires Apple silicon macOS")
}
