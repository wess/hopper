use super::{checkpoint, optical, storage};
use anyhow::{ensure, Context};
use machine::{
  devices::virtio::{gpu, pci},
  hypervisor::{self as hv, state::group},
  runtime::{self, Boot, Control, Mode},
};
use std::time::{Duration, Instant};

pub(super) fn devices() -> anyhow::Result<[Option<pci::Device>; 7]> {
  let mut devices = std::array::from_fn(|_| None);
  devices[0] = Some(storage::device()?);
  devices[1] = Some(checkpoint::graphics()?);
  devices[2] = Some(checkpoint::device()?);
  devices[3] = Some(optical::device()?);
  Ok(devices)
}

fn counts(ram: &hv::Memory<'_>) -> anyhow::Result<[u64; 2]> {
  let mut bytes = [0; 16];
  hv::read(ram, 0x10000, &mut bytes)?;
  Ok([
    u64::from_le_bytes(bytes[..8].try_into()?),
    u64::from_le_bytes(bytes[8..].try_into()?),
  ])
}

pub fn run(saved: &mut group::Checkpoint, boot: &impl Fn() -> Boot) -> anyhow::Result<()> {
  let mut original = [0; 16];
  saved.primary.memory.ram.read(0x10000, &mut original)?;
  let expected = [
    u64::from_le_bytes(original[..8].try_into()?),
    u64::from_le_bytes(original[8..].try_into()?),
  ];
  let mut wrong = boot();
  wrong.cpus = 1;
  ensure!(
    runtime::run_restored(wrong, devices()?, saved, |_, _, _| panic!(
      "Rejected recovery reached execution callback"
    ))
    .is_err()
  );
  ensure!(runtime::run_restored(
    boot(),
    std::array::from_fn(|_| None),
    saved,
    |_, _, _| panic!("Rejected recovery reached execution callback")
  )
  .is_err());
  let mut wrong = boot();
  wrong.memory *= 2;
  ensure!(
    runtime::run_restored(wrong, devices()?, saved, |_, _, _| panic!(
      "Rejected recovery reached execution callback"
    ))
    .is_err()
  );
  saved.cpus.swap(0, 1);
  let result = runtime::run_restored(boot(), devices()?, saved, |_, _, _| {
    panic!("Rejected recovery reached execution callback")
  });
  saved.cpus.swap(0, 1);
  ensure!(result.is_err(), "Foreign CPU affinity was accepted");
  let interrupts = std::mem::replace(
    &mut saved.primary.interrupts,
    hv::gic::state::State::from_bytes(&[0; 8])?,
  );
  let invalid = runtime::run_restored(boot(), devices()?, saved, |_, _, _| {
    panic!("Rejected recovery reached execution callback")
  });
  saved.primary.interrupts = interrupts;
  ensure!(invalid.is_err(), "Invalid GIC state was accepted");
  let result = runtime::run_restored(boot(), devices()?, saved, |_, _, mode| {
    ensure!(mode == Mode::Paused);
    anyhow::bail!("recovery callback failure")
  });
  ensure!(result
    .err()
    .is_some_and(|error| error.to_string() == "recovery callback failure"));
  super::persistent::run(saved, boot, devices)?;
  let started = Instant::now();
  let stopped = runtime::run_restored(boot(), devices()?, saved, |_, ram, mode| {
    ensure!(mode == Mode::Paused && counts(ram)? == expected);
    Ok(Control::Stop)
  })?;
  ensure!(
    stopped.console == saved.primary.actions.tail() && started.elapsed() < Duration::from_secs(2)
  );
  drop(stopped);
  for _ in 0..2 {
    let mut checked = false;
    let mut resumed = false;
    let mut since = Instant::now();
    let stopped = runtime::run_restored(boot(), devices()?, saved, |devices, ram, mode| {
      if !resumed {
        ensure!(
          mode == Mode::Paused && counts(ram)? == expected,
          "Guest executed before recovery release"
        );
        if !checked {
          ensure!(pci::completed(devices[2].as_ref().context("Missing keyboard")?) == 2);
          let display = pci::display(devices[1].as_ref().context("Missing graphics")?)
            .context("Missing display")?;
          ensure!(gpu::frame(display).context("Missing frame")?.rgba == [3, 2, 1, 255].repeat(4));
          storage::verify(devices[0].as_mut().context("Missing disk")?, ram)?;
          optical::verify(devices[3].as_mut().context("Missing optical")?, ram)?;
          checked = true;
          since = Instant::now();
        }
        if since.elapsed() < Duration::from_millis(100) {
          return Ok(Control::Pause);
        }
        resumed = true;
        since = Instant::now();
        return Ok(Control::Continue);
      }
      if mode == Mode::Running && since.elapsed() >= Duration::from_millis(100) {
        return Ok(Control::Pause);
      }
      if mode == Mode::Paused {
        ensure!(
          counts(ram)?
            .iter()
            .zip(expected)
            .all(|(&new, old)| new > old),
          "Recovered CPU did not continue its saved loop"
        );
        return Ok(Control::Stop);
      }
      Ok(Control::Continue)
    })?;
    ensure!(checked && resumed && stopped.reason == runtime::Reason::Stopped);
    ensure!(
      !stopped.console.is_empty() && stopped.console.iter().all(|&byte| byte == b'A'),
      "Recovered secondary CPU did not continue UART traps"
    );
    drop(stopped);
  }
  saved.primary.memory.ram.read(0x10000, &mut original)?;
  ensure!(
    [
      u64::from_le_bytes(original[..8].try_into()?),
      u64::from_le_bytes(original[8..].try_into()?)
    ] == expected
  );
  println!("Fresh runtime restoration preserves held memory/devices, resumes both saved CPU loops and UART traps, supports repeated independent recovery and rejects topology/affinity errors");
  Ok(())
}

pub fn single(saved: &group::Checkpoint, boot: &impl Fn() -> Boot) -> anyhow::Result<()> {
  let mut config = boot();
  config.cpus = 1;
  let mut released = false;
  let stopped = runtime::run_restored(
    config,
    std::array::from_fn(|_| None),
    saved,
    |_, ram, mode| {
      let mut count = [0; 8];
      hv::read(ram, 0x10000, &mut count)?;
      if !released {
        ensure!(mode == Mode::Paused && count == [0; 8]);
        released = true;
      } else if count != [0; 8] {
        return Ok(Control::Stop);
      }
      Ok(Control::Continue)
    },
  )?;
  ensure!(released && stopped.reason == runtime::Reason::Stopped);
  drop(stopped);
  let mut count = [0; 8];
  saved.primary.memory.ram.read(0x10000, &mut count)?;
  ensure!(count == [0; 8]);
  println!("Single-CPU runtime reconstruction starts held, resumes the saved guest and leaves captured RAM unchanged");
  Ok(())
}
