pub fn run() -> anyhow::Result<()> {
  use anyhow::{ensure, Context};
  use machine::{
    devices::{flash, serial},
    hypervisor::{self as hv, state::group},
    runtime::{self, Boot, Control, Mode},
  };
  let code = [
    0xd2a08004u32,
    0xd2a80025,
    0xd2a12006,
    0x52806020,
    0xb90030c0,
    0x52800200,
    0xb9002cc0,
    0x52824680,
    0xb90024c0,
    0x52800420,
    0xb90028c0,
    0x52800820,
    0xb90000c0,
    0x52801d00,
    0xb9000080,
    0x52800020,
    0xb9000080,
    0x528acf00,
    0x72a24680,
    0xb9000080,
    0xd2800020,
    0xf90000a0,
    0xf94004a0,
    0xb4ffffe0,
    0x529de020,
    0x72b579a0,
    0xb9000480,
    0x52801a00,
    0xb9000080,
    0xd2800040,
    0xf90000a0,
    0x14000000,
  ];
  let boot = || Boot {
    firmware: code.into_iter().flat_map(u32::to_le_bytes).collect(),
    variables: vec![0xff],
    memory: 0x10000000,
    cpus: 1,
    timeout: std::time::Duration::from_secs(5),
  };
  let directory = tempfile::tempdir()?;
  let root = directory.path().join("variables");
  let mut store = runtime::variables::create(&root, &[0xff])?;
  let mut persistent = boot();
  persistent.variables = runtime::variables::bytes(&store).to_vec();
  let group = group::create(1)?;
  let mut phase = 0;
  let mut pending = None;
  let mut saved = None;
  let stopped = runtime::run_persistent_captured(
    persistent,
    std::array::from_fn(|_| None),
    &mut store,
    group.clone(),
    |_, ram, mode| {
      let mut marker = [0; 8];
      hv::read(ram, 0x10000, &mut marker)?;
      match (phase, mode) {
        (0, Mode::Running) if u64::from_le_bytes(marker) == 1 => {
          phase = 1;
          Ok(Control::Pause)
        }
        (1, Mode::Paused) => {
          pending = Some(group.request()?);
          phase = 2;
          Ok(Control::Pause)
        }
        (2, Mode::Paused) => {
          if let Some(captured) = pending.as_mut().context("Missing flash capture")?.poll()? {
            saved = Some(captured);
            hv::write(ram, 0x10008, &1u64.to_le_bytes())?;
            phase = 3;
            return Ok(Control::Continue);
          }
          Ok(Control::Pause)
        }
        (3, Mode::Running) if u64::from_le_bytes(marker) == 2 => Ok(Control::Stop),
        _ => Ok(Control::Continue),
      }
    },
  )?;
  ensure!(stopped.reason == runtime::Reason::Stopped && phase == 3);
  ensure!(stopped.console == b"A");
  ensure!(&stopped.variables[..8] == 0xabcdef0112345678u64.to_le_bytes());
  ensure!(stopped.variables == runtime::variables::bytes(&store));
  drop(stopped);
  drop(store);
  let mut store = runtime::variables::open(&root)?;
  ensure!(runtime::variables::bytes(&store)[..8] == 0xabcdef0112345678u64.to_le_bytes());
  let captured = super::records::reopen(saved.context("Flash capture did not complete")?, || {
    Ok(std::array::from_fn(|_| None))
  })?;
  ensure!(runtime::run_restored_persistent(
    boot(),
    std::array::from_fn(|_| None),
    &mut store,
    &captured,
    |_, _, _| panic!("Mismatched durable source reached hardware callbacks"),
  )
  .is_err());
  let restored_root = directory.path().join("restored");
  let mut durable = runtime::variables::from_snapshot(
    &restored_root,
    &captured.primary.memory.nvram,
    &captured.primary.devices.flash,
    || Ok(()),
  )?;
  ensure!(
    runtime::variables::bytes(&durable)
      .iter()
      .all(|byte| *byte == 0xff),
    "Pending NOR bytes entered the restored store before guest confirmation"
  );
  let mut resumed = false;
  let restored = runtime::run_restored_persistent_captured(
    boot(),
    std::array::from_fn(|_| None),
    &mut durable,
    &captured,
    group::create(1)?,
    |_, ram, mode| {
      let mut marker = [0; 8];
      hv::read(ram, 0x10000, &mut marker)?;
      if !resumed {
        ensure!(mode == Mode::Paused && u64::from_le_bytes(marker) == 1);
        hv::write(ram, 0x10008, &1u64.to_le_bytes())?;
        resumed = true;
      } else if u64::from_le_bytes(marker) == 2 {
        return Ok(Control::Stop);
      }
      Ok(Control::Continue)
    },
  )?;
  ensure!(resumed && restored.reason == runtime::Reason::Stopped);
  ensure!(&restored.variables[..8] == 0xabcdef0112345678u64.to_le_bytes());
  ensure!(
    restored.variables == runtime::variables::bytes(&durable),
    "Restored guest NOR writes were not durably acknowledged"
  );
  drop(restored);
  drop(durable);
  let reopened = runtime::variables::open(&restored_root)?;
  ensure!(
    runtime::variables::bytes(&reopened)[..8] == 0xabcdef0112345678u64.to_le_bytes(),
    "Restored guest NOR writes were lost on reopen"
  );
  let nvram = &captured.primary.memory.nvram;
  let mut data = vec![0; nvram.size()];
  for (index, chunk) in data.chunks_mut(65536).enumerate() {
    nvram.read(index * 65536, chunk)?;
  }
  ensure!(
    data.iter().all(|byte| *byte == 0xff),
    "Pending flash writes were published early"
  );
  let mut bank = flash::snapshot::restore(&captured.primary.devices.flash, data)?;
  ensure!(!flash::array(&bank));
  flash::write(&mut bank, 4, 0xfedcba98)?;
  ensure!(flash::write(&mut bank, 0, 0xd0)? == Some(0..8));
  flash::write(&mut bank, 0, 0xff)?;
  ensure!(flash::read(&bank, 0, 8)? == 0xfedcba9812345678);
  let mut console = captured.primary.devices.console.clone();
  for (offset, expected) in [(0x30, 0x301), (0x2c, 0x10), (0x24, 0x1234), (0x28, 0x21)] {
    ensure!(
      serial::read(&mut console, offset) == expected,
      "Guest UART state was lost"
    );
  }
  ensure!(serial::write(&mut console, 0, b'B'.into()) == Some(b'B'));
  ensure!(
    runtime::variables::bytes(&store)[..8] == 0xabcdef0112345678u64.to_le_bytes(),
    "Independent captured flash changed the durable source"
  );
  println!("Persistent source and restored NOR commit/reopen, pending-write isolation, mismatched durable-store rejection and guest UART restoration passed");
  Ok(())
}
