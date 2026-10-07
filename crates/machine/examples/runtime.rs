#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn main() -> anyhow::Result<()> {
  use anyhow::{ensure, Context};
  use machine::{
    devices::virtio::{console, gpu, input, pci, scsi},
    runtime::{self, Boot, Control},
    setup,
  };
  use std::{
    io::Write,
    path::Path,
    time::{Duration, Instant},
  };
  fn export(display: &gpu::Frame, path: &Path) -> anyhow::Result<()> {
    let mut file = std::io::BufWriter::new(
      std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?,
    );
    write!(file, "P6\n{} {}\n255\n", display.width, display.height)?;
    for pixel in display.rgba.as_chunks::<4>().0 {
      file.write_all(&pixel[..3])?;
    }
    file.flush()?;
    Ok(())
  }
  let args: Vec<_> = std::env::args().collect();
  ensure!(
    matches!(args.len(), 6 | 7),
    "Provide firmware, variables, boot DVD, installer DVD, new frame path and optional variable store"
  );
  let mut boot = Boot {
    firmware: std::fs::read(&args[1])?,
    variables: std::fs::read(&args[2])?,
    memory: 0x100000000,
    cpus: 2,
    timeout: Duration::from_secs(75),
  };
  let mut store = args
    .get(6)
    .map(|root| -> anyhow::Result<_> {
      let root = Path::new(root);
      if root.exists() {
        runtime::variables::open(root)
      } else {
        runtime::variables::create(root, &boot.variables)
      }
    })
    .transpose()?;
  if let Some(store) = &store {
    boot.variables = runtime::variables::bytes(store).to_vec();
  }
  let devices = [
    Some(pci::optical(scsi::open(Path::new(&args[3]))?)?),
    Some(pci::graphics(gpu::create(1024, 768)?)?),
    Some(pci::controller(input::create(input::Kind::Keyboard))?),
    Some(pci::controller(input::create(input::Kind::Tablet))?),
    None,
    Some(pci::optical(scsi::open(Path::new(&args[4]))?)?),
    Some(pci::serial(console::create("org.hopper.setup")?)?),
  ];
  let mut decoder = setup::Decoder::default();
  let mut events = 0;
  let start = Instant::now();
  let mut captured = 0;
  let mut held_frame = Vec::new();
  let poll = |devices: &mut [Option<pci::Device>; 7],
              ram: &mut machine::hypervisor::Memory<'_>,
              mode: runtime::Mode| {
    let bytes = pci::receive_serial(
      devices[6].as_mut().context("Missing setup controller")?,
      ram,
    )?;
    for event in setup::feed(&mut decoder, &bytes)? {
      events += 1;
      eprintln!("Setup event: {event:?}");
    }
    if mode == runtime::Mode::Paused {
      let sampled = Instant::now();
      pci::refresh(devices[1].as_mut().context("Missing display")?, ram)?;
      let display = pci::display(devices[1].as_ref().context("Missing display")?)
        .and_then(gpu::frame)
        .context("Paused runtime produced no frame")?;
      let suffix = if captured == 0 {
        "paused.ppm"
      } else {
        "resumed.ppm"
      };
      export(display, &Path::new(&args[5]).with_extension(suffix))?;
      held_frame = display.rgba.clone();
      captured += 1;
      eprintln!(
        "Captured guest frame {captured} with both CPUs paused in {:?}",
        sampled.elapsed()
      );
      return Ok(if captured == 2 {
        Control::Stop
      } else {
        Control::Continue
      });
    }
    Ok(if start.elapsed() >= Duration::from_secs(55) {
      Control::Stop
    } else if (start.elapsed() >= Duration::from_secs(30) && captured == 0)
      || (start.elapsed() >= Duration::from_secs(50) && captured == 1)
    {
      Control::Pause
    } else {
      Control::Continue
    })
  };
  let stopped = if let Some(store) = &mut store {
    runtime::run_persistent(boot, devices, store, poll)?
  } else {
    runtime::run(boot, devices, poll)?
  };
  if let Some(store) = &store {
    ensure!(
      runtime::variables::bytes(store) == stopped.variables,
      "Persistent firmware state differs from the stopped guest"
    );
  }
  drop(store);
  if let Some(root) = args.get(6) {
    let reopened = runtime::variables::open(Path::new(root))?;
    ensure!(
      runtime::variables::bytes(&reopened) == stopped.variables,
      "Firmware changes did not survive reopening"
    );
    eprintln!("Persistent firmware state verified after reopening");
  }
  ensure!(
    captured == 2,
    "Native runtime did not capture a paused frame"
  );
  ensure!(
    stopped.reason == runtime::Reason::Stopped,
    "Unexpected guest power event"
  );
  ensure!(
    setup::finish(&mut decoder)? == setup::Status::Failed(setup::Phase::Partition),
    "Read-only setup check did not reach its expected terminal phase"
  );
  eprintln!("Native runtime stopped cleanly after {events} setup events");
  ensure!(
    stopped.variables.len() == 0x4000000,
    "Variable bank was not retained"
  );
  for (slot, device) in stopped.devices.iter().enumerate() {
    if let Some(device) = device {
      ensure!(pci::fault(device).is_none(), "Native device {slot} fault");
      eprintln!(
        "Device {slot} completed {} requests",
        pci::completed(device)
      );
    }
  }
  let display = pci::display(stopped.devices[1].as_ref().context("Missing display")?)
    .and_then(gpu::frame)
    .context("Native runtime produced no frame")?;
  ensure!(
    display.rgba == held_frame,
    "Framebuffer changed while stopping a paused guest"
  );
  export(display, Path::new(&args[5]))?;
  Ok(())
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("Native execution requires an Apple silicon Mac")
}
