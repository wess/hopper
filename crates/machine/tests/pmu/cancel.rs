use super::{context, prepare};
use crate::hypervisor::{self as hv, state};
use anyhow::{ensure, Context};
use std::{
  sync::atomic::{AtomicBool, Ordering},
  time::Duration,
};

#[test]
#[ignore = "requires a signed Hypervisor-entitled test executable"]
fn interrupted_access() -> anyhow::Result<()> {
  let vm = hv::create()?;
  let _gic = hv::gic::create(&vm, 0x08000000, 0x0a000000)?;
  let code = prepare(&vm, 0x3fff0000)?;
  let access = code.token();
  let mut cpu = hv::cpu(&vm)?;
  hv::affinity(&mut cpu, 0)?;
  hv::enter(&mut cpu, 0x40000000)?;
  hv::set(&mut cpu, 0, 0x12345678)?;
  let architectural = state::capture(&cpu)?;
  let baseline = access.capture(&mut cpu)?;
  let wake = hv::wake(&cpu);
  let stop = AtomicBool::new(false);
  let mut completed = 0;
  let mut bounded = 0;
  std::thread::scope(|scope| -> anyhow::Result<()> {
    let interrupt = scope.spawn(|| -> anyhow::Result<usize> {
      let mut requests = 0;
      while !stop.load(Ordering::Acquire) {
        ensure!(hv::request_exit(&wake)?, "PMU test CPU was destroyed");
        requests += 1;
        std::thread::sleep(Duration::from_micros(50));
      }
      Ok(requests)
    });
    let result = (|| {
      for _ in 0..1024 {
        match access.capture(&mut cpu) {
          Ok(saved) => {
            completed += 1;
            for index in [0, 1, 2, 3, 5, 6, 7] {
              ensure!(
                saved.values[index] == baseline.values[index],
                "Interrupted PMU read changed control {index}"
              );
            }
          }
          Err(error)
            if error.to_string() == "PMU access was repeatedly interrupted"
              || format!("{error:#}").contains("Guest execution deadline expired") =>
          {
            bounded += 1;
          }
          Err(error) => return Err(error).context("Interrupted PMU register capture"),
        }
        ensure!(
          state::capture(&cpu)? == architectural,
          "Interrupted PMU capture changed CPU context"
        );
      }
      Ok(())
    })();
    stop.store(true, Ordering::Release);
    let requests = interrupt
      .join()
      .map_err(|_| anyhow::anyhow!("PMU interrupt test panicked"))??;
    ensure!(requests > 0, "PMU interruption was not exercised");
    result?;
    println!("Interrupted PMU reads: {completed} complete, {bounded} bounded rejections, {requests} exit requests");
    Ok(())
  })?;
  ensure!(completed > 0);
  let final_state = access.capture(&mut cpu)?;
  for index in [0, 1, 2, 3, 5, 6, 7] {
    ensure!(
      final_state.values[index] == baseline.values[index],
      "PMU controls changed after interruptions"
    );
  }
  ensure!(state::capture(&cpu)? == architectural);
  Ok(())
}

#[test]
#[ignore = "requires a signed Hypervisor-entitled test executable"]
fn canceled_checkpoint_restarts_fixed_access() -> anyhow::Result<()> {
  let vm = hv::create()?;
  let _gic = hv::gic::create(&vm, 0x08000000, 0x0a000000)?;
  let code = prepare(&vm, 0x3fff0000)?;
  let access = code.token();
  let mut cpu = hv::cpu(&vm)?;
  hv::affinity(&mut cpu, 0)?;
  hv::enter(&mut cpu, 0x40000000)?;
  let original = state::capture(&cpu)?;
  context::run(&mut cpu, access.address + 0x800, |cpu| {
    let baseline = access.access(cpu, 0xdce0, None)?;
    let mut interrupted = false;
    let result = access.access_with(cpu, 0xdce0, None, |cpu| {
      let exit = hv::run(cpu)?;
      if !interrupted && matches!(exit, hv::Exit::Exception { .. }) {
        interrupted = true;
        // model an external cancellation after the access has reached its HVC checkpoint.
        return Ok(hv::Exit::Canceled);
      }
      Ok(exit)
    })?;
    ensure!(
      interrupted && result == baseline,
      "Canceled read did not retry its own access"
    );
    let previous = access.access(cpu, 0xdce5, None)?;
    let written = (|| {
      let mut interrupted = false;
      access.access_with(cpu, 0xdce5, Some(7), |cpu| {
        let exit = hv::run(cpu)?;
        if !interrupted && matches!(exit, hv::Exit::Exception { .. }) {
          interrupted = true;
          hv::set(cpu, 0, 0)?;
          return Ok(hv::Exit::Canceled);
        }
        Ok(exit)
      })?;
      ensure!(
        interrupted && access.access(cpu, 0xdce5, None)? == 7,
        "Canceled write did not restart with its original operand"
      );
      Ok::<_, anyhow::Error>(())
    })();
    access.access(cpu, 0xdce5, Some(previous))?;
    written?;
    let mut attempts = 0;
    let rejected = access.access_with(cpu, 0xdce0, None, |cpu| {
      let _ = hv::run(cpu)?;
      attempts += 1;
      Ok(hv::Exit::Canceled)
    });
    ensure!(
      attempts == 16
        && rejected
          .err()
          .is_some_and(|error| error.to_string() == "PMU access was repeatedly interrupted"),
      "Repeated cancellation did not reject at its bound"
    );
    Ok(())
  })?;
  ensure!(state::capture(&cpu)? == original);
  Ok(())
}

#[test]
#[ignore = "requires a signed Hypervisor-entitled test executable"]
fn checkpoint_fallthrough_stays_in_access() -> anyhow::Result<()> {
  let vm = hv::create()?;
  let _gic = hv::gic::create(&vm, 0x08000000, 0x0a000000)?;
  let code = prepare(&vm, 0x3fff0000)?;
  let access = code.token();
  let mut cpu = hv::cpu(&vm)?;
  hv::affinity(&mut cpu, 0)?;
  hv::enter(&mut cpu, 0x40000000)?;
  let original = state::capture(&cpu)?;
  context::run(&mut cpu, access.address + 0x800, |cpu| {
    let control = access.access(cpu, 0xdce0, None)?;
    let selection = access.access(cpu, 0xdce5, None)?;
    let result = (|| {
      for completed in [false, true] {
        for (register, operand) in [(0xdce0, None), (0xdce5, Some(7))] {
          let mut skipped = false;
          let value = access.access_with(cpu, register, operand, |cpu| {
            if !skipped {
              // cover both direct continuation and suppression of a real completed HVC exit.
              if completed {
                ensure!(matches!(hv::run(cpu)?, hv::Exit::Exception { syndrome, .. }
                  if crate::arm::decode(syndrome) == crate::arm::Trap::Hypercall(0)));
              } else {
                let pc = hv::get(cpu, 31)?;
                hv::set(cpu, 31, pc + 8)?;
              }
              skipped = true;
            }
            hv::run(cpu)
          })?;
          ensure!(skipped, "Checkpoint fallthrough was not exercised");
          if operand.is_none() {
            ensure!(value == control, "Fallthrough changed the captured control");
          } else {
            ensure!(
              access.access(cpu, register, None)? == 7,
              "Fallthrough lost the write"
            );
          }
        }
      }
      ensure!(access.access(cpu, 0xdce0, None)? == control);
      Ok::<_, anyhow::Error>(())
    })();
    access.access(cpu, 0xdce5, Some(selection))?;
    result
  })?;
  ensure!(state::capture(&cpu)? == original);
  Ok(())
}
