#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn main() -> anyhow::Result<()> {
  use anyhow::{ensure, Context};
  use machine::vz::{self, network::Mode, sharing::Directory, Action, Boot, Linux};
  use objc2::MainThreadMarker;
  use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSRunLoop};
  use std::{
    os::unix::fs::OpenOptionsExt,
    time::{Duration, Instant},
  };

  let args: Vec<_> = std::env::args_os().skip(1).collect();
  ensure!(
    args.len() == 2,
    "Provide diagnostic ARM64 kernel and sharing initramfs"
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
  let writable = root.path().join("writable");
  let readonly = root.path().join("readonly");
  std::fs::create_dir(&writable)?;
  std::fs::create_dir(&readonly)?;
  std::fs::write(writable.join("sentinel"), "authorized-writable")?;
  std::fs::write(readonly.join("sentinel"), "authorized-readonly")?;
  let outside = root.path().join("outside");
  std::fs::write(&outside, "outside-scope")?;
  std::os::unix::fs::symlink(&outside, writable.join("escape"))?;
  std::os::unix::fs::symlink("../../outside", writable.join("relative"))?;
  let shares = vec![
    Directory::open("writable", &writable, false)?,
    Directory::open("readonly", &readonly, true)?,
  ];
  let identity = vz::identity();
  let run_loop = NSRunLoop::currentRunLoop();
  for present in [false, true] {
    let (mode, option, marker) = (Mode::Disconnected, "sharing", "HOPPER_SHARING_OK");
    let output = root.path().join(format!("{option}-{present}"));
    let console = std::fs::OpenOptions::new()
      .create_new(true)
      .write(true)
      .mode(0o600)
      .open(&output)?;
    let mut boot = Linux {
      cpus: 2,
      memory: 512 << 20,
      width: 1024,
      height: 768,
      identity: identity.clone(),
      boot: Boot::Kernel {
        kernel: args[0].clone().into(),
        initramfs: Some(args[1].clone().into()),
        command_line: format!(
          "console=hvc0 rdinit=/init hopper.sharing={}",
          if present { "present" } else { "absent" }
        ),
      },
      disk: disk.clone(),
      installer: None,
      seed: None,
      network: Some(mode),
      shares: shares.clone(),
      speakers: false,
      console: Some(console),
    };
    if present {
      boot.shares = vec![shares[0].clone(); 17];
      ensure!(
        vz::create(main, &boot).is_err(),
        "Excessive shared folders were admitted"
      );
      boot.shares = vec![shares[0].clone(); 2];
      ensure!(
        vz::create(main, &boot).is_err(),
        "Duplicate shared folder names were admitted"
      );
      boot.shares = shares.clone();
    } else {
      boot.shares = Vec::new();
    }
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
    let moved = root.path().join("moved");
    if present {
      std::fs::rename(&writable, &moved)?;
      std::fs::create_dir(&writable)?;
      std::fs::write(writable.join("sentinel"), "replacement-scope")?;
      ensure!(
        vz::transition(&vm, Action::Start).is_err(),
        "Replaced share was admitted"
      );
      std::fs::remove_dir_all(&writable)?;
      std::fs::rename(&moved, &writable)?;
    }
    wait(vz::transition(&vm, Action::Start)?)?;
    let mut replaced = false;
    let result = (|| -> anyhow::Result<()> {
      let deadline = Instant::now() + Duration::from_secs(60);
      loop {
        ensure!(
          std::fs::metadata(&output)?.len() <= 1 << 20,
          "Serial output exceeds bounds"
        );
        let text = std::fs::read_to_string(&output)?;
        if (!present && text.contains("HOPPER_SHARING_ABSENT_OK"))
          || (text.contains("HOPPER_REPLACEMENT_OK") && text.contains("HOPPER_USER_SHARING_OK"))
        {
          return Ok(());
        }
        if present && text.contains(marker) && !replaced {
          std::fs::rename(&writable, &moved)?;
          std::fs::create_dir(&writable)?;
          std::fs::write(writable.join("sentinel"), "replacement-scope")?;
          std::fs::write(readonly.join("check"), "check")?;
          replaced = true;
        }
        if text.contains("HOPPER_SHARING_FAILED") || Instant::now() >= deadline {
          anyhow::bail!(
            "Guest sharing probe failed: {}",
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
    if !present {
      println!(
        "No shared-folder device: guest startup condition skipped without creating a mount point"
      );
      continue;
    }
    ensure!(
      std::fs::read_to_string(moved.join("created"))? == "guest-write",
      "Guest write did not reach authorized directory"
    );
    ensure!(
      std::fs::read_to_string(moved.join("usercreated"))? == "guest-user-write",
      "Unprivileged guest write did not reach the authorized directory"
    );
    ensure!(
      !readonly.join("created").exists() && !readonly.join("usercreated").exists(),
      "Read-only folder changed"
    );
    ensure!(
      !writable.join("created").exists() && !writable.join("after").exists(),
      "Guest wrote into replacement directory"
    );
    println!("Named shares, unprivileged guest reads/writes, read-only enforcement, replacement-path isolation and guest symlink scope verified");
  }
  println!("Native directory sharing probe completed; no OS installation was performed");
  Ok(())
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("VZ probe requires Apple silicon macOS")
}
