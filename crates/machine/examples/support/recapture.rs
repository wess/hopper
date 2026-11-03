use super::{optical, recovery, storage};
use anyhow::{ensure, Context};
use machine::{
  hypervisor::{self as hv, state::group},
  runtime::{self, Boot, Control, Mode},
};

fn counts(ram: &hv::Memory<'_>) -> anyhow::Result<[u64; 2]> {
  let mut bytes = [0; 16];
  hv::read(ram, 0x10000, &mut bytes)?;
  decode(bytes)
}

fn decode(bytes: [u8; 16]) -> anyhow::Result<[u64; 2]> {
  Ok([
    u64::from_le_bytes(bytes[..8].try_into()?),
    u64::from_le_bytes(bytes[8..].try_into()?),
  ])
}

pub fn run(saved: &group::Checkpoint, boot: &impl Fn() -> Boot) -> anyhow::Result<()> {
  let mut original = [0; 16];
  saved.primary.memory.ram.read(0x10000, &mut original)?;
  let expected = decode(original)?;
  let group = group::create(2)?;
  let mut phase = 0;
  let mut pending = None;
  let mut captured = None;
  let stopped = runtime::run_restored_captured(
    boot(),
    recovery::devices()?,
    saved,
    group.clone(),
    |_, ram, mode| match (phase, mode) {
      (0, Mode::Paused) => {
        ensure!(counts(ram)? == expected && group.request().is_err());
        phase = 1;
        Ok(Control::Continue)
      }
      (1, Mode::Running)
        if counts(ram)?
          .iter()
          .zip(expected)
          .all(|(&new, old)| new > old) =>
      {
        Ok(Control::Pause)
      }
      (1, Mode::Paused) => {
        pending = Some(group.request()?);
        phase = 2;
        Ok(Control::Pause)
      }
      (2, Mode::Paused) => {
        if let Some(saved) = pending
          .as_mut()
          .context("Missing recapture request")?
          .poll()?
        {
          captured = Some(saved);
          phase = 3;
          return Ok(Control::Stop);
        }
        Ok(Control::Pause)
      }
      _ => Ok(Control::Continue),
    },
  )?;
  ensure!(phase == 3 && stopped.reason == runtime::Reason::Stopped && group.request().is_err());
  drop(stopped);
  let captured = captured.context("Restored VM did not produce a checkpoint")?;
  let mut bytes = [0; 16];
  captured.primary.memory.ram.read(0x10000, &mut bytes)?;
  let held = decode(bytes)?;
  ensure!(held.iter().zip(expected).all(|(&new, old)| new > old));
  for _ in 0..2 {
    let mut resumed = false;
    let stopped = runtime::run_restored(
      boot(),
      recovery::devices()?,
      &captured,
      |devices, ram, mode| {
        if !resumed {
          ensure!(mode == Mode::Paused && counts(ram)? == held);
          storage::verify(devices[0].as_mut().context("Missing recaptured disk")?, ram)?;
          optical::verify(
            devices[3].as_mut().context("Missing recaptured optical")?,
            ram,
          )?;
          resumed = true;
        } else if counts(ram)?.iter().zip(held).all(|(&new, old)| new > old) {
          return Ok(Control::Stop);
        }
        Ok(Control::Continue)
      },
    )?;
    ensure!(resumed && stopped.reason == runtime::Reason::Stopped);
    drop(stopped);
  }
  saved.primary.memory.ram.read(0x10000, &mut bytes)?;
  ensure!(bytes == original);
  println!("Restored runtime recapture survives source teardown, independently recovers disk/media and resumes both CPUs repeatedly without changing the original checkpoint");
  Ok(())
}
