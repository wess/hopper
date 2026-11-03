#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn main() -> anyhow::Result<()> {
  use anyhow::{ensure, Context};
  use fs2::FileExt;
  use machine::{
    devices::virtio::{block, console, gpu, input, pci},
    runtime::{self, variables, Boot, Control},
  };
  use std::{
    fs::OpenOptions,
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::Path,
    time::{Duration, Instant},
  };
  let args: Vec<_> = std::env::args().collect();
  ensure!(
    args.len() == 6,
    "Provide firmware, variable store, disposable disk copy, disk identity and new frame path"
  );
  let store = variables::open(Path::new(&args[2]))?;
  let boot = Boot {
    firmware: std::fs::read(&args[1])?,
    variables: variables::bytes(&store).to_vec(),
    memory: 0x100000000,
    cpus: 2,
    timeout: Duration::from_secs(90),
  };
  let file = OpenOptions::new()
    .read(true)
    .write(true)
    .custom_flags(libc::O_NOFOLLOW)
    .open(&args[3])?;
  file
    .try_lock_exclusive()
    .context("Diagnostic disk is in use")?;
  ensure!(
    file.metadata()?.is_file(),
    "Diagnostic disk must be regular"
  );
  let devices = [
    None,
    Some(pci::graphics(gpu::create(1024, 768)?)?),
    Some(pci::controller(input::create(input::Kind::Keyboard))?),
    Some(pci::controller(input::create(input::Kind::Tablet))?),
    Some(pci::create(block::attach(
      file,
      false,
      args[4].as_bytes().try_into()?,
    )?)?),
    None,
    Some(pci::serial(console::create("org.hopper.setup")?)?),
  ];
  let start = Instant::now();
  let stopped = runtime::run(boot, devices, |_, _, _| {
    Ok(if start.elapsed() >= Duration::from_secs(60) {
      Control::Stop
    } else {
      Control::Continue
    })
  })?;
  eprintln!(
    "Guest stopped after {:?}: {:?}",
    start.elapsed(),
    stopped.reason
  );
  eprintln!(
    "Bounded UART tail: {:?}",
    String::from_utf8_lossy(&stopped.console)
  );
  for (slot, device) in stopped.devices.iter().enumerate() {
    if let Some(device) = device {
      eprintln!(
        "Device {slot}: {} requests, fault {:?}",
        pci::completed(device),
        pci::fault(device)
      );
    }
  }
  if let Some(frame) = stopped.devices[1]
    .as_ref()
    .and_then(pci::display)
    .and_then(gpu::frame)
  {
    let mut file = OpenOptions::new()
      .write(true)
      .create_new(true)
      .mode(0o600)
      .open(&args[5])?;
    write!(file, "P6\n{} {}\n255\n", frame.width, frame.height)?;
    for pixel in frame.rgba.as_chunks::<4>().0 {
      file.write_all(&pixel[..3])?;
    }
    file.flush()?;
    eprintln!("Stopped display saved to {}", args[5]);
  } else {
    eprintln!("Guest produced no display");
  }
  Ok(())
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("Native execution requires an Apple silicon Mac")
}
