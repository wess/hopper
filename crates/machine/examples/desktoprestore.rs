#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "support/frame.rs"]
mod frame;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn main() -> anyhow::Result<()> {
  use anyhow::{ensure, Context};
  use fs2::FileExt;
  use machine::{
    devices::virtio::{block, console, gpu, input, pci},
    hypervisor::state::group,
    runtime::{self, variables, Boot, Control, Mode},
  };
  use sha2::{Digest, Sha256};
  use std::{
    fs::OpenOptions,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
    time::{Duration, Instant},
  };

  let args = std::env::args().collect::<Vec<_>>();
  ensure!(
    args.len() == 4 || (args.len() == 5 && matches!(args[4].as_str(), "save" | "load")),
    "Provide firmware, private disposable diagnostic root and disk identity"
  );
  let root = std::fs::canonicalize(&args[2])?;
  let metadata = std::fs::symlink_metadata(&root)?;
  ensure!(
    root.parent() == Some(Path::new("/private/tmp")) || root.parent() == Some(Path::new("/tmp")),
    "Use a temporary diagnostic root"
  );
  ensure!(
    root
      .file_name()
      .is_some_and(|name| name.to_string_lossy().starts_with("hopperrecovery119."))
      && metadata.mode() & 0o777 == 0o700
      && metadata.uid() == unsafe { libc::geteuid() },
    "Use a private disposable recovery fixture"
  );
  let identity: [u8; 20] = args[3].as_bytes().try_into()?;
  let store = variables::open(&root.join("variables"))?;
  let firmware = std::fs::read(&args[1])?;
  let boot = || Boot {
    firmware: firmware.clone(),
    variables: variables::bytes(&store).to_vec(),
    memory: 0x100000000,
    cpus: 2,
    timeout: Duration::from_secs(100),
  };
  let devices = || -> anyhow::Result<[Option<pci::Device>; 7]> {
    let file = OpenOptions::new()
      .read(true)
      .write(true)
      .custom_flags(libc::O_NOFOLLOW)
      .open(root.join("disk"))?;
    file
      .try_lock_exclusive()
      .context("Disposable Windows disk is in use")?;
    Ok([
      None,
      Some(pci::graphics(gpu::create(1024, 768)?)?),
      Some(pci::controller(input::create(input::Kind::Keyboard))?),
      Some(pci::controller(input::create(input::Kind::Tablet))?),
      Some(pci::create(block::attach(file, false, identity)?)?),
      None,
      Some(pci::serial(console::create("org.hopper.setup")?)?),
    ])
  };
  let image = |devices: &[Option<pci::Device>; 7]| -> anyhow::Result<[u8; 32]> {
    let frame = devices[1]
      .as_ref()
      .and_then(pci::display)
      .and_then(gpu::frame)
      .context("Missing Windows display")?;
    Ok(Sha256::digest(&frame.rgba).into())
  };
  let export = |devices: &mut [Option<pci::Device>; 7], name: &str| -> anyhow::Result<()> {
    frame::export(
      devices[1].as_mut().context("Missing graphics")?,
      &[
        "--frame".into(),
        root.join(name).to_string_lossy().into_owned(),
      ],
    )
  };
  let mut configuration = Sha256::new();
  configuration.update(b"hopper.desktop.fixture.v1");
  configuration.update(&firmware);
  configuration.update(identity);
  configuration.update(variables::bytes(&store));
  let configuration: [u8; 32] = configuration.finalize().into();
  let (saved, expected, requests) = if args.get(4).is_some_and(|mode| mode == "load") {
    let hardware = devices()?;
    record::load(&root, &hardware, configuration)?
  } else {
    let group = group::create(2)?;
    let mut pending = None;
    let mut captured = None;
    let mut expected = None;
    let started = Instant::now();
    let mut stopped =
      runtime::run_captured(boot(), devices()?, group.clone(), |devices, ram, mode| {
        if mode == Mode::Paused {
          if pending.is_none() {
            pci::refresh(devices[1].as_mut().context("Missing graphics")?, ram)?;
            export(devices, "boot.ppm")?;
            expected = Some(image(devices)?);
            pending = Some(group.request()?);
            eprintln!(
              "Windows running capture requested after {:?}",
              started.elapsed()
            );
          }
          if let Some(saved) = pending
            .as_mut()
            .context("Missing capture request")?
            .poll()?
          {
            captured = Some(saved);
            return Ok(Control::Stop);
          }
          return Ok(Control::Pause);
        }
        Ok(if started.elapsed() >= Duration::from_secs(60) {
          Control::Pause
        } else {
          Control::Continue
        })
      })?;
    ensure!(
      stopped.reason == runtime::Reason::Stopped,
      "Windows boot ended before capture: {:?}",
      stopped.reason
    );
    export(&mut stopped.devices, "source.ppm")?;
    let requests = stopped
      .devices
      .each_ref()
      .map(|device| device.as_ref().map(pci::completed));
    eprintln!("Windows boot requests: {requests:?}");
    drop(stopped);
    let saved = captured.context("Windows capture did not complete")?;
    let expected = expected.context("Missing captured display hash")?;
    if args.get(4).is_some_and(|mode| mode == "save") {
      record::save(&root, &saved, expected, requests, configuration)?;
      drop(saved);
      println!("Installed Windows checkpoint published; source hardware and captured objects destroyed before process exit");
      return Ok(());
    }
    (saved, expected, requests)
  };
  eprintln!("Windows source hardware destroyed; reconstructing captured state");
  let restored_at = Instant::now();
  let mut resumed = None;
  let mut sent = false;
  let mut stopped = runtime::run_restored(boot(), devices()?, &saved, |devices, ram, mode| {
    if resumed.is_none() {
      ensure!(
        mode == Mode::Paused && image(devices)? == expected,
        "Restored Windows display does not match captured state"
      );
      export(devices, "held.ppm")?;
      resumed = Some(Instant::now());
      eprintln!(
        "Windows reconstruction completed in {:?}; captured display matches",
        restored_at.elapsed()
      );
    }
    let elapsed = resumed.context("Missing resume time")?.elapsed();
    if elapsed >= Duration::from_secs(10) && !sent {
      // synthetic input targets only this disposable guest; no credentials are entered.
      pci::send_input(
        devices[2].as_mut().context("Missing keyboard")?,
        ram,
        &[
          input::Event {
            kind: 1,
            code: 57,
            value: 1,
          },
          input::SYN,
          input::Event {
            kind: 1,
            code: 57,
            value: 0,
          },
          input::SYN,
        ],
      )?;
      sent = true;
    }
    Ok(if elapsed >= Duration::from_secs(30) {
      Control::Stop
    } else {
      Control::Continue
    })
  })?;
  ensure!(
    stopped.reason == runtime::Reason::Stopped,
    "Recovered Windows stopped unexpectedly: {:?}",
    stopped.reason
  );
  export(&mut stopped.devices, "recovered.ppm")?;
  for (slot, device) in stopped.devices.iter().enumerate() {
    if let Some(device) = device {
      ensure!(
        pci::fault(device).is_none(),
        "Recovered device {slot} faulted"
      );
      eprintln!(
        "Recovered device {slot}: {} requests (before: {:?})",
        pci::completed(device),
        requests[slot]
      );
    }
  }
  println!("Windows source booted, captured, reconstructed into fresh hardware with matching held pixels and ran for thirty seconds; inspect frames for desktop/input readiness. Windows-specific PMU semantics and application preservation remain unverified.");
  Ok(())
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("Native desktop recovery requires Apple silicon macOS")
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "support/desktoprecord.rs"]
mod record;
