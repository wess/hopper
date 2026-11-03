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
    args.len() == 4,
    "Provide verified ARM64 kernel, inspection initramfs, stopped guest disk and matching modloop"
  );
  let main = MainThreadMarker::new().context("Guest inspection requires the main thread")?;
  let root = tempfile::Builder::new()
    .prefix("hopperinspect90.")
    .tempdir_in("/tmp")?
    .keep();
  eprintln!("Read-only inspection evidence: {}", root.display());
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
    installer: Some(args[2].clone().into()),
    seed: Some(args[3].clone().into()),
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
      if text.contains("HOPPER_INSPECT_DONE") {
        return Ok(());
      }
      if text.contains("HOPPER_INSPECT_FAILED") || Instant::now() >= deadline {
        anyhow::bail!(
          "Read-only inspection failed: {}",
          text.lines().rev().take(30).collect::<Vec<_>>().join("\n")
        );
      }
      tick();
    }
  })();
  let stopped = wait(vz::transition(&vm, Action::Stop)?);
  result?;
  stopped?;
  println!(
    "Read-only guest inspection finished; evidence at {}",
    output.display()
  );
  Ok(())
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("VZ guest inspection requires Apple silicon macOS")
}
