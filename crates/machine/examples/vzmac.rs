#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn main() -> anyhow::Result<()> {
  use anyhow::{ensure, Context};
  use machine::vz::{self, mac, restore};
  use objc2::MainThreadMarker;
  use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSRunLoop};
  use objc2_virtualization::VZVirtualMachineState;
  use std::{
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    time::{Duration, Instant},
  };

  let main = MainThreadMarker::new().context("macOS VZ probe requires the main thread")?;
  let discovery = restore::latest();
  let run_loop = NSRunLoop::currentRunLoop();
  let deadline = Instant::now() + Duration::from_secs(120);
  let image = loop {
    if let Some(result) = restore::poll(&discovery)? {
      break result?;
    }
    ensure!(
      Instant::now() < deadline,
      "macOS restore discovery timed out"
    );
    run_loop.runMode_beforeDate(
      unsafe { NSDefaultRunLoopMode },
      &NSDate::dateWithTimeIntervalSinceNow(0.01),
    );
  };
  println!(
    "Supported macOS {}.{}.{} build {}; minimum {} CPUs, {} bytes RAM",
    image.version[0],
    image.version[1],
    image.version[2],
    image.build,
    image.minimum_cpus,
    image.minimum_memory
  );
  let root = tempfile::tempdir()?;
  std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))?;
  let invalid = root.path().join("invalid.ipsw");
  std::fs::write(&invalid, b"invalid restore image")?;
  let inspection = restore::local(&invalid)?;
  let deadline = Instant::now() + Duration::from_secs(30);
  loop {
    if let Some(result) = restore::poll(&inspection)? {
      ensure!(
        result.is_err(),
        "Invalid local restore image must be rejected"
      );
      break;
    }
    ensure!(
      Instant::now() < deadline,
      "Local restore inspection timed out"
    );
    run_loop.runMode_beforeDate(
      unsafe { NSDefaultRunLoopMode },
      &NSDate::dateWithTimeIntervalSinceNow(0.01),
    );
  }
  ensure!(
    std::fs::read(&invalid)? == b"invalid restore image",
    "Restore inspection changed media"
  );
  let link = root.path().join("linked.ipsw");
  std::os::unix::fs::symlink(&invalid, &link)?;
  ensure!(
    restore::local(&link).is_err(),
    "Restore symlinks must be rejected"
  );
  ensure!(
    restore::local(root.path()).is_err(),
    "Restore directories must be rejected"
  );
  let auxiliary = root.path().join("auxiliary");
  mac::create_auxiliary(&auxiliary, &image.hardware)?;
  let original = std::fs::read(auxiliary.join("state"))?;
  ensure!(
    mac::create_auxiliary(&auxiliary, &image.hardware).is_err(),
    "Existing macOS auxiliary storage must not be replaced"
  );
  ensure!(
    std::fs::read(auxiliary.join("state"))? == original,
    "Rejected creation changed existing auxiliary storage"
  );
  let disk = root.path().join("disk");
  std::fs::OpenOptions::new()
    .write(true)
    .create_new(true)
    .mode(0o600)
    .open(&disk)?
    .set_len(64 << 30)?;
  let mut boot = mac::Mac {
    cpus: image.minimum_cpus,
    memory: image.minimum_memory,
    width: 1024,
    height: 768,
    image,
    identity: mac::identity(),
    auxiliary,
    disk,
    network: vz::network::Mode::Nat,
  };
  for mode in [vz::network::Mode::Nat, vz::network::Mode::Disconnected] {
    boot.network = mode;
    let vm = vz::create_mac(main, &boot)?;
    ensure!(
      vz::network::attachments(&vm) == vec![mode == vz::network::Mode::Nat],
      "macOS VZ network attachment does not match its configuration"
    );
    ensure!(
      vz::state(&vm) == VZVirtualMachineState::Stopped,
      "Network configuration started uninstalled macOS hardware"
    );
  }
  boot.network = vz::network::Mode::Nat;
  let mut vm = vz::create_mac(main, &boot)?;
  ensure!(
    vz::state(&vm) == VZVirtualMachineState::Stopped,
    "Configuration must not start macOS hardware"
  );
  for attempt in 0..2 {
    let installation = vz::install::start(&mut vm, &invalid)?;
    if attempt == 1 {
      vz::install::cancel(&installation);
    }
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
      ensure!(
        (0.0..=1.0).contains(&vz::install::fraction(&installation)),
        "Installer progress exceeds bounds"
      );
      if let Some(result) = vz::install::poll(&installation)? {
        ensure!(
          result.is_err(),
          "Installer must reject malformed restore media"
        );
        break;
      }
      ensure!(
        Instant::now() < deadline,
        "Invalid installation did not complete"
      );
      run_loop.runMode_beforeDate(
        unsafe { NSDefaultRunLoopMode },
        &NSDate::dateWithTimeIntervalSinceNow(0.01),
      );
    }
    drop(installation);
    let error = vz::transition(&vm, vz::Action::Start)
      .err()
      .context("Failed or cancelled installation allowed macOS startup")?;
    ensure!(
      error.to_string().contains("Install macOS"),
      "Unexpected post-install startup rejection: {error}"
    );
  }
  ensure!(
    std::fs::read(&invalid)? == b"invalid restore image",
    "Installer changed restore input"
  );
  drop(vm);
  let cpus = boot.cpus;
  boot.cpus = cpus.saturating_sub(1);
  ensure!(
    vz::create_mac(main, &boot).is_err(),
    "Restore CPU minimum must be enforced"
  );
  boot.cpus = cpus;
  let memory = boot.memory;
  boot.memory = memory.saturating_sub(1);
  ensure!(
    vz::create_mac(main, &boot).is_err(),
    "Restore memory minimum must be enforced"
  );
  boot.memory = memory;
  std::fs::write(boot.auxiliary.join("hardware"), b"another model")?;
  let error = vz::create_mac(main, &boot)
    .err()
    .context("Mismatched auxiliary storage must be rejected")?;
  ensure!(
    error.to_string().contains("different macOS hardware model"),
    "Wrong auxiliary rejection: {error}"
  );
  ensure!(
    std::fs::read(boot.auxiliary.join("state"))? == original,
    "Mismatched model changed auxiliary storage"
  );
  println!("macOS restore discovery, NAT/disconnected network devices, invalid local media/installation rejection, configuration and auxiliary binding verified; no IPSW was downloaded or installed");
  Ok(())
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("macOS VZ probe requires Apple silicon macOS")
}
