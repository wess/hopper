#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "support/current.rs"]
mod current;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "support/parked.rs"]
mod parked;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "support/checkpoint.rs"]
mod checkpoint;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "support/flash.rs"]
mod flash;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "support/storage.rs"]
mod storage;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "support/optical.rs"]
mod optical;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "support/recovery.rs"]
mod recovery;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "support/generationattempts.rs"]
mod generationattempts;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "support/generationfailures.rs"]
mod generationfailures;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "support/generation.rs"]
mod generation;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "support/persistentcapture.rs"]
mod persistentcapture;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "support/persistent.rs"]
mod persistent;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "support/recapture.rs"]
mod recapture;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "support/actions.rs"]
mod actions;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn main() -> anyhow::Result<()> {
  use anyhow::{ensure, Context};
  use machine::{
    hypervisor::state::group,
    runtime::{self, Boot, Control, Mode},
  };
  use std::time::{Duration, Instant};

  let bytes = |code: &[u32]| {
    code
      .iter()
      .flat_map(|word| word.to_le_bytes())
      .collect::<Vec<_>>()
  };
  let mut firmware = bytes(&[
    0x52800060, 0x72b88000, 0xd2800021, 0xd2802002, 0xd2800003, 0xd4000002, 0xd2a80024, 0xd2a12027,
    0xb94000e8, 0x11384108, 0xb90008e8, 0xb94000e8, 0xb9002088, 0xf9400085, 0x910004a5, 0xf9000085,
    0x17fffffb,
  ]);
  firmware.resize(0x100, 0);
  firmware.extend(bytes(&[
    0xd2a80024, 0xd510109f, 0x91002084, 0xd2a12006, 0x52806028, 0xb90030c8, 0x52800827, 0xf9400085,
    0x910004a5, 0xb90000c7, 0xf9000085, 0x17fffffc,
  ]));
  let boot = || Boot {
    firmware: firmware.clone(),
    variables: vec![0xff],
    memory: 0x10000000,
    cpus: 2,
    timeout: Duration::from_secs(5),
  };
  let operation = process::operation()?;
  if let Some(process::Operation::Load(parent)) = &operation {
    return process::load(parent, &boot);
  }
  let empty = || std::array::from_fn(|_| None);
  let group = group::create(2)?;
  ensure!(group.request().is_err());
  let mut pending: Option<group::Capture> = None;
  let mut baseline: Option<group::Checkpoint> = None;
  let mut publication = None;
  let mut published = false;
  let publisher_resumed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
  let mut held = [0; 2];
  let mut phase = 0;
  let mut since = Instant::now();
  let mut devices = empty();
  devices[0] = Some(storage::device()?);
  devices[1] = Some(checkpoint::graphics()?);
  devices[2] = Some(checkpoint::device()?);
  devices[3] = Some(optical::device()?);
  let binding = process::binding(&boot(), &devices, || Ok(()))?;
  let persistent = tempfile::tempdir()?;
  let mut store = runtime::variables::create(&persistent.path().join("variables"), &[0xff])?;
  let mut prepared = boot();
  prepared.variables = runtime::variables::bytes(&store).to_vec();
  let stopped = runtime::run_persistent_captured(
    prepared,
    devices,
    &mut store,
    group.clone(),
    |devices, ram, mode| match (phase, mode) {
      (0, Mode::Running)
        if since.elapsed() >= Duration::from_millis(100)
          && counts(ram)?.iter().all(|&count| count > 0) =>
      {
        phase = 1;
        Ok(Control::Pause)
      }
      (1, Mode::Paused) => {
        held = counts(ram)?;
        ensure!(held.iter().all(|&count| count > 0));
        optical::prepare(devices[3].as_mut().context("Missing optical media")?, ram)?;
        storage::prepare(devices[0].as_mut().context("Missing disk")?, ram)?;
        checkpoint::prepare_transport(devices[2].as_mut().context("Missing keyboard")?, ram)?;
        checkpoint::prepare_graphics(devices[1].as_mut().context("Missing graphics")?, ram)?;
        drop(group.request()?);
        pending = Some(group.request()?);
        ensure!(group.request().is_err());
        machine::hypervisor::write(ram, 0x20000, &123u64.to_le_bytes())?;
        phase = 2;
        Ok(Control::Pause)
      }
      (2, Mode::Paused) => {
        ensure!(counts(ram)? == held, "Guest RAM changed during capture");
        if let Some(captured) = pending.as_mut().context("Missing capture")?.poll()? {
          checkpoint::verify(&captured, held)?;
          ensure!(captured.cpus.len() == 2 && captured.cpus[0].clock == captured.cpus[1].clock);
          ensure!(
            captured.cpus[0].cpu != captured.cpus[1].cpu,
            "CPU affinity was lost"
          );
          ensure!(
            captured.primary.software.power().count() == 2
              && captured.primary.software.power().online(0)?
              && captured.primary.software.power().online(1)?
          );
          let mut primary = captured.primary.software.debug(0)?;
          let mut secondary = captured.primary.software.debug(1)?;
          let status = machine::arm::SystemAccess {
            encoding: 0x808c,
            register: 0,
            read: true,
          };
          ensure!(machine::debug::access(&mut primary, status, 0) == Some(10));
          ensure!(
            machine::debug::access(&mut secondary, status, 0) == Some(8),
            "Guest debug unlock was lost"
          );
          baseline = Some(captured);
          pending = Some(group.request()?);
          phase = 3;
        }
        Ok(Control::Pause)
      }
      (3, Mode::Paused) => {
        ensure!(counts(ram)? == held);
        if let Some(captured) = pending
          .as_mut()
          .context("Missing repeated capture")?
          .poll()?
        {
          let baseline = baseline.as_ref().context("Missing baseline")?;
          checkpoint::verify(&captured, held)?;
          checkpoint::verify(baseline, held)?;
          ensure!(
            captured
              .cpus
              .iter()
              .zip(&baseline.cpus)
              .all(|(new, old)| new.cpu == old.cpu),
            "Held CPU registers changed"
          );
          pending = Some(group.request()?);
          phase = 4;
          since = Instant::now();
          if let Some(process::Operation::Save(parent)) = &operation {
            publication = Some(process::publish(
              parent,
              captured,
              binding,
              publisher_resumed.clone(),
            )?);
          }
          optical::change(devices[3].as_mut().context("Missing optical media")?, ram)?;
          storage::change(devices[0].as_mut().context("Missing disk")?, ram)?;
          checkpoint::change_graphics(devices[1].as_mut().context("Missing graphics")?, ram)?;
          return Ok(Control::Continue);
        }
        Ok(Control::Pause)
      }
      (4, Mode::Running) => {
        checkpoint::verify(baseline.as_ref().context("Missing baseline")?, held)?;
        ensure!(pending
          .as_mut()
          .context("Missing cancelled capture")?
          .poll()
          .is_err());
        ensure!(group.request().is_err());
        if counts(ram)?.iter().zip(held).all(|(new, old)| *new > old) {
          publisher_resumed.store(true, std::sync::atomic::Ordering::Release);
        }
        if !published && publisher_resumed.load(std::sync::atomic::Ordering::Acquire) {
          if let Some(writer) = publication.as_mut() {
            if let Some(package) = writer.poll()? {
              ensure!(publisher_resumed.load(std::sync::atomic::Ordering::Acquire));
              drop(package);
              published = true;
              println!("Background checkpoint publication completed while both source CPUs continued after resume");
            }
          }
        }
        if since.elapsed() >= Duration::from_millis(100) && (publication.is_none() || published) {
          phase = 5;
          Ok(Control::Pause)
        } else {
          Ok(Control::Continue)
        }
      }
      (5, Mode::Paused) => {
        ensure!(counts(ram)?.iter().zip(held).all(|(new, old)| *new > old));
        pending = Some(group.request()?);
        phase = 6;
        Ok(Control::Pause)
      }
      (6, Mode::Paused) => {
        if pending
          .as_mut()
          .context("Missing new-generation capture")?
          .poll()?
          .is_some()
        {
          pending = Some(group.request()?);
          phase = 7;
          return Ok(Control::Stop);
        }
        Ok(Control::Pause)
      }
      _ => Ok(Control::Continue),
    },
  )?;
  ensure!(phase == 7 && stopped.reason == runtime::Reason::Stopped);
  ensure!(
    !stopped.console.is_empty() && stopped.console.iter().all(|&byte| byte == b'A'),
    "Secondary CPU did not complete real UART traps"
  );
  ensure!(stopped.variables == runtime::variables::bytes(&store));
  drop(stopped);
  drop(store);
  if let Some(process::Operation::Save(_)) = &operation {
    ensure!(
      published,
      "Background checkpoint publication did not complete"
    );
    drop(publication.take());
    drop(baseline.take());
    println!("Complete two-CPU checkpoint published; source hardware and captured objects destroyed before process exit");
    return Ok(());
  }
  let mut saved = records::reopen(
    baseline.take().context("Missing retained checkpoint")?,
    recovery::devices,
  )?;
  let saved = &mut saved;
  checkpoint::verify(saved, held)?;
  checkpoint::restore_interrupts(saved)?;
  checkpoint::restore_device(saved)?;
  storage::restore(saved)?;
  optical::restore(saved)?;
  recovery::run(saved, &boot)?;
  recapture::run(saved, &boot)?;
  ensure!(pending
    .as_mut()
    .context("Missing stopped capture")?
    .poll()
    .is_err());
  ensure!(group.request().is_err());
  ensure!(runtime::run_captured(boot(), empty(), group, |_, _, _| Ok(Control::Stop)).is_err());
  let single = group::create(1)?;
  let mut config = boot();
  config.cpus = 1;
  config.firmware = bytes(&[0xd2a80024, 0xf9400085, 0x910004a5, 0xf9000085, 0x17fffffd]);
  let mut pending = None;
  let mut complete = false;
  let mut single_saved = None;
  let stopped = runtime::run_captured(config, empty(), single.clone(), |_, _, mode| {
    if mode == Mode::Paused {
      if pending.is_none() {
        pending = Some(single.request()?);
      }
      let capture = pending.as_mut().context("Missing single-CPU capture")?;
      if let Some(captured) = capture.poll()? {
        ensure!(captured.cpus.len() == 1);
        complete = true;
        single_saved = Some(captured);
        return Ok(Control::Stop);
      }
    }
    Ok(Control::Pause)
  })?;
  ensure!(complete && single.request().is_err());
  drop(stopped);
  recovery::single(
    &single_saved.context("Missing single-CPU checkpoint")?,
    &boot,
  )?;
  let failed = group::create(2)?;
  let mut pending = None;
  let error = runtime::run_captured(boot(), empty(), failed.clone(), |_, _, mode| {
    if mode == Mode::Paused {
      pending = Some(failed.request()?);
      anyhow::bail!("capture callback failure");
    }
    Ok(Control::Pause)
  });
  ensure!(error
    .err()
    .is_some_and(|error| error.to_string() == "capture callback failure"));
  ensure!(pending
    .as_mut()
    .context("Missing failed capture")?
    .poll()
    .is_err());
  ensure!(failed.request().is_err());
  let final_group = group::create(2)?;
  let stopped = runtime::run_captured(boot(), empty(), final_group.clone(), |_, _, _| {
    Ok(Control::Stop)
  })?;
  drop(stopped);
  ensure!(final_group.request().is_err());
  actions::run()?;
  parked::run()?;
  flash::run()?;
  println!("Two-CPU capture: stable RAM/registers, shared clock epoch, repeated capture, cancellation, resume invalidation, new pause and runtime retirement passed");
  println!("Single-CPU capture, callback failure invalidation and VM reacquisition passed");
  println!("Coordinated RAM/firmware/NVRAM/GIC capture, immutable retention across resume/teardown and fresh GIC restoration passed");
  Ok(())
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn counts(ram: &machine::hypervisor::Memory<'_>) -> anyhow::Result<[u64; 2]> {
  let mut bytes = [0; 16];
  machine::hypervisor::read(ram, 0x10000, &mut bytes)?;
  Ok([
    u64::from_le_bytes(bytes[..8].try_into()?),
    u64::from_le_bytes(bytes[8..].try_into()?),
  ])
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("Native execution requires Apple silicon macOS")
}
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "support/cpurecords.rs"]
mod records;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "support/process.rs"]
mod process;
