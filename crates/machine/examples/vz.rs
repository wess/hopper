#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn main() -> anyhow::Result<()> {
  use anyhow::{ensure, Context};
  use machine::vz::{self, Action, Linux};
  use objc2::MainThreadMarker;
  use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSRunLoop};
  use objc2_virtualization::VZVirtualMachineState as State;
  use std::{
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    time::{Duration, Instant},
  };

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
  let variables = root.path().join("variables");
  vz::create_variables(&variables)?;
  let original = std::fs::read(&variables)?;
  ensure!(
    vz::create_variables(&variables).is_err(),
    "Existing EFI variables must not be overwritten"
  );
  ensure!(
    std::fs::read(&variables)? == original,
    "Rejected EFI creation changed existing data"
  );
  ensure!(
    std::fs::metadata(&variables)?.permissions().mode() & 0o777 == 0o600,
    "EFI variables must remain private"
  );
  let mut boot = Linux {
    cpus: 2,
    memory: 512 << 20,
    width: 1024,
    height: 768,
    identity: vz::identity(),
    boot: vz::Boot::Efi { variables },
    disk,
    installer: None,
    console: None,
  };
  let legacy = root.path().join("previous");
  std::fs::write(&legacy, b"QFI\xfbretained prior image")?;
  let raw = std::mem::replace(&mut boot.disk, legacy.clone());
  ensure!(
    vz::create(main, &boot).is_err(),
    "Previous image formats must require migration"
  );
  ensure!(
    std::fs::read(&legacy)? == b"QFI\xfbretained prior image",
    "Rejected migration changed the previous image"
  );
  boot.disk = raw;
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
    println!("VZ state: {state:?}");
  }
  println!("VZ hardware transitions verified; no guest OS was installed or booted");
  Ok(())
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("VZ probe requires Apple silicon macOS")
}
