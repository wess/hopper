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
  let args: Vec<_> = std::env::args().collect();
  ensure!(
    args.len() == 6,
    "Provide firmware, variables, boot DVD, installer DVD and new frame path"
  );
  let boot = Boot {
    firmware: std::fs::read(&args[1])?,
    variables: std::fs::read(&args[2])?,
    memory: 0x100000000,
    cpus: 2,
    timeout: Duration::from_secs(75),
  };
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
  let stopped = runtime::run(boot, devices, |devices, ram| {
    let bytes = pci::receive_serial(
      devices[6].as_mut().context("Missing setup controller")?,
      ram,
    )?;
    for event in setup::feed(&mut decoder, &bytes)? {
      events += 1;
      eprintln!("Setup event: {event:?}");
    }
    Ok(if start.elapsed() >= Duration::from_secs(55) {
      Control::Stop
    } else {
      Control::Continue
    })
  })?;
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
  let mut file = std::io::BufWriter::new(
    std::fs::OpenOptions::new()
      .write(true)
      .create_new(true)
      .open(&args[5])?,
  );
  write!(file, "P6\n{} {}\n255\n", display.width, display.height)?;
  for pixel in display.rgba.as_chunks::<4>().0 {
    file.write_all(&pixel[..3])?;
  }
  file.flush()?;
  Ok(())
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("Native execution requires an Apple silicon Mac")
}
