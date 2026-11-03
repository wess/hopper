use anyhow::{ensure, Context};
use machine::{
  devices::virtio::pci,
  hypervisor::{
    self as hv,
    state::group::{self, Checkpoint},
  },
  runtime::{self, variables, Boot, Control, Mode},
};
use std::{fs::OpenOptions, os::unix::fs::FileExt};

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

fn retired(root: &std::path::Path, stopped: runtime::Stopped, expected: u8) -> anyhow::Result<()> {
  let file = OpenOptions::new()
    .read(true)
    .write(true)
    .open(super::generation::disk(root))?;
  ensure!(
    fs2::FileExt::try_lock_exclusive(&file).is_err(),
    "Returned devices lost adopted ownership"
  );
  drop(stopped);
  fs2::FileExt::try_lock_exclusive(&file)?;
  fs2::FileExt::unlock(&file)?;
  let mut bytes = [0; 512];
  file.read_exact_at(&mut bytes, 0)?;
  ensure!(
    bytes == [expected; 512],
    "Restored virtio writes missed the persistent disk"
  );
  Ok(())
}

pub fn run(
  original: &Checkpoint,
  boot: &impl Fn() -> Boot,
  devices: &impl Fn() -> anyhow::Result<[Option<pci::Device>; 7]>,
) -> anyhow::Result<()> {
  let mut bytes = [0; 16];
  original.primary.memory.ram.read(0x10000, &mut bytes)?;
  let baseline = bytes;
  let expected = decode(bytes)?;
  let directory = tempfile::tempdir()?;
  let (restored, mut store, disks) =
    super::generation::resources(directory.path(), original, true, devices)?;
  let owners = group::create(2)?;
  let mut phase = 0;
  let mut pending = None;
  let mut captured = None;
  let stopped = runtime::recovery::run_recovered_captured(
    boot(),
    devices()?,
    &mut store,
    &restored,
    disks,
    owners.clone(),
    |hardware, ram, mode| match (phase, mode) {
      (0, Mode::Paused) => {
        ensure!(counts(ram)? == expected && owners.request().is_err());
        super::storage::verify(hardware[0].as_mut().context("Missing adopted disk")?, ram)?;
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
        pending = Some(owners.request()?);
        phase = 2;
        Ok(Control::Pause)
      }
      (2, Mode::Paused) => {
        if let Some(saved) = pending
          .as_mut()
          .context("Missing persistent recapture")?
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
  ensure!(phase == 3 && stopped.reason == runtime::Reason::Stopped && owners.request().is_err());
  ensure!(
    stopped.variables == variables::bytes(&store),
    "Recapture lost durable EFI consistency"
  );
  retired(directory.path(), stopped, b'c')?;
  drop(store);
  super::generation::replay(directory.path(), original, devices, boot, b'c')?;
  let captured = super::records::reopen(captured.context("No persistent checkpoint")?, devices)?;
  captured.primary.memory.ram.read(0x10000, &mut bytes)?;
  let held = decode(bytes)?;
  ensure!(held.iter().zip(expected).all(|(&new, old)| new > old));
  for _ in 0..2 {
    let root = tempfile::tempdir()?;
    let (reopened, mut store, disks) =
      super::generation::resources(root.path(), &captured, false, devices)?;
    let mut released = false;
    let stopped = runtime::recovery::run_recovered(
      boot(),
      devices()?,
      &mut store,
      &reopened,
      disks,
      |hardware, ram, mode| {
        if !released {
          ensure!(mode == Mode::Paused && counts(ram)? == held);
          super::storage::verify_recaptured(
            hardware[0].as_mut().context("Missing recaptured disk")?,
            ram,
          )?;
          released = true;
          return Ok(Control::Continue);
        }
        if mode == Mode::Running && counts(ram)?.iter().zip(held).all(|(&new, old)| new > old) {
          return Ok(Control::Pause);
        }
        if mode == Mode::Paused {
          return Ok(Control::Stop);
        }
        Ok(Control::Continue)
      },
    )?;
    ensure!(released && stopped.reason == runtime::Reason::Stopped);
    retired(root.path(), stopped, b'd')?;
    drop(store);
    super::generation::replay(root.path(), &captured, devices, boot, b'd')?;
  }
  original.primary.memory.ram.read(0x10000, &mut bytes)?;
  ensure!(
    bytes == baseline,
    "Persistent recapture changed the original RAM cut"
  );
  println!("Persistent restored runtime recaptures durable disk/EFI state after both CPUs advance, seals/reopens after teardown, repeatedly recovers independent writable disks and preserves the original checkpoint");
  Ok(())
}
