#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "support/frame.rs"]
mod frame;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn main() -> anyhow::Result<()> {
  use anyhow::{ensure, Context};
  use machine::{
    devices::virtio::{console, gpu, input, pci, scsi},
    runtime::{self, Boot, Control},
    setup,
  };
  use std::{
    path::Path,
    time::{Duration, Instant},
  };
  let args: Vec<_> = std::env::args().collect();
  ensure!(
    args.len() == 5,
    "Provide firmware, variable template, disposable handshake-only WinPE ISO and new frame path"
  );
  let boot = Boot {
    firmware: std::fs::read(&args[1])?,
    variables: std::fs::read(&args[2])?,
    memory: 0x100000000,
    cpus: 2,
    timeout: Duration::from_secs(90),
  };
  let devices = [
    Some(pci::optical(scsi::open(Path::new(&args[3]))?)?),
    Some(pci::graphics(gpu::create(1024, 768)?)?),
    Some(pci::controller(input::create(input::Kind::Keyboard))?),
    Some(pci::controller(input::create(input::Kind::Tablet))?),
    None,
    None,
    Some(pci::serial(console::create("org.hopper.setup")?)?),
  ];
  let mut decoder = setup::Decoder::default();
  let mut sent = false;
  let mut attempted = false;
  let start = Instant::now();
  let mut stopped = runtime::run(boot, devices, |devices, ram, _| {
    let port = devices[6].as_mut().context("Missing setup port")?;
    let bytes = pci::receive_serial(port, ram)?;
    for event in setup::feed(&mut decoder, &bytes)? {
      eprintln!("Setup: {event:?}");
    }
    if setup::status(&decoder) == setup::Status::Deployed && !sent {
      match pci::send_serial(port, ram, b"HOPPER_SHUTDOWN\r\n") {
        Ok(()) => {
          sent = true;
          eprintln!("Shutdown acknowledgement delivered");
        }
        Err(error) if !attempted => {
          attempted = true;
          eprintln!("First acknowledgement attempt: {error:#}");
        }
        Err(_) => {}
      }
    }
    Ok(if start.elapsed() > Duration::from_secs(60) {
      Control::Stop
    } else {
      Control::Continue
    })
  })?;
  eprintln!(
    "Stopped after {:?}: {:?}; acknowledgement sent: {sent}",
    start.elapsed(),
    stopped.reason
  );
  eprintln!("UART tail: {:?}", String::from_utf8_lossy(&stopped.console));
  frame::export(
    stopped.devices[1].as_mut().context("Missing display")?,
    &["--frame".into(), args[4].clone()],
  )?;
  ensure!(
    stopped.reason == runtime::Reason::Shutdown && sent,
    "WinPE handshake did not shut down cleanly"
  );
  Ok(())
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("Native execution requires Apple silicon macOS")
}
