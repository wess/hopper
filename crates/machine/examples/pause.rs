#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn main() -> anyhow::Result<()> {
  use anyhow::ensure;
  use machine::{
    hypervisor as hv,
    runtime::{self, Boot, Control, Mode, Reason},
  };
  use std::time::{Duration, Instant};

  fn boot(firmware: Vec<u8>) -> Boot {
    Boot {
      firmware,
      variables: vec![0xff],
      memory: 0x10000000,
      cpus: 2,
      timeout: Duration::from_secs(5),
    }
  }
  fn bytes(code: &[u32]) -> Vec<u8> {
    code.iter().flat_map(|word| word.to_le_bytes()).collect()
  }
  fn counts(ram: &hv::Memory<'_>) -> anyhow::Result<[u64; 2]> {
    let mut bytes = [0; 16];
    hv::read(ram, 0x10000, &mut bytes)?;
    Ok([
      u64::from_le_bytes(bytes[..8].try_into()?),
      u64::from_le_bytes(bytes[8..].try_into()?),
    ])
  }
  let empty = || std::array::from_fn(|_| None);
  // primary starts CPU 1 at flash offset 0x100; each CPU increments its own RAM word.
  let mut firmware = bytes(&[
    0x52800060, 0x72b88000, 0xd2800021, 0xd2802002, 0xd2800003, 0xd4000002, 0xd2a80024, 0xd503201f,
    0xf9400085, 0x910004a5, 0xf9000085, 0x17fffffd,
  ]);
  firmware.resize(0x100, 0);
  firmware.extend(bytes(&[
    0xd2a80024, 0xd503201f, 0x91002084, 0xf9400085, 0x910004a5, 0xf9000085, 0x17fffffd,
  ]));
  let mut phase = 0;
  let mut since = None;
  let mut held = [0; 2];
  let stopped = runtime::run(boot(firmware), empty(), |_, ram, mode| {
    let since = since.get_or_insert_with(Instant::now);
    match (phase, mode) {
      (0, Mode::Running) if since.elapsed() >= Duration::from_millis(100) => {
        phase = 1;
        *since = Instant::now();
        Ok(Control::Pause)
      }
      (1, Mode::Paused) => {
        let current = counts(ram)?;
        if held == [0; 2] {
          ensure!(
            current.iter().all(|value| *value > 0),
            "Both guest CPUs must be running"
          );
          held = current;
        }
        ensure!(
          current == held,
          "Guest RAM changed while all CPUs were paused"
        );
        if since.elapsed() >= Duration::from_millis(100) {
          phase = 2;
          *since = Instant::now();
          Ok(Control::Continue)
        } else {
          Ok(Control::Pause)
        }
      }
      (2, Mode::Running) if since.elapsed() >= Duration::from_millis(100) => {
        phase = 3;
        Ok(Control::Pause)
      }
      (3, Mode::Paused) => {
        let current = counts(ram)?;
        ensure!(
          current.iter().zip(held).all(|(new, old)| *new > old),
          "Both guest CPUs must resume execution"
        );
        phase = 4;
        Ok(Control::Stop)
      }
      _ => Ok(Control::Continue),
    }
  })?;
  ensure!(
    phase == 4 && stopped.reason == Reason::Stopped,
    "Coordinated pause did not complete"
  );
  drop(stopped);
  let idle = || bytes(&[0x14000000]);
  let error = runtime::run(boot(idle()), empty(), |_, _, mode| {
    if mode == Mode::Paused {
      anyhow::bail!("paused callback check");
    }
    Ok(Control::Pause)
  });
  ensure!(
    error
      .err()
      .is_some_and(|error| error.to_string() == "paused callback check"),
    "Paused error did not propagate"
  );
  let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    runtime::run(boot(idle()), empty(), |_, _, mode| {
      assert!(mode != Mode::Paused, "intentional paused callback panic");
      Ok(Control::Pause)
    })
  }));
  ensure!(panic.is_err(), "Paused panic check did not unwind");
  let stopped = runtime::run(boot(idle()), empty(), |_, _, _| Ok(Control::Stop))?;
  ensure!(
    stopped.reason == Reason::Stopped,
    "Native VM could not be reacquired after paused teardown"
  );
  eprintln!("Native coordinated pause verified: two active CPUs, stable RAM, resume, paused stop/error/panic and reacquisition");
  Ok(())
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("Native execution requires an Apple silicon Mac")
}
