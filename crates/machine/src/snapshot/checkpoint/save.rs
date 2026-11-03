use super::layout;
use crate::{
  devices::{
    flash, rtc, serial,
    virtio::pci::snapshot::{backendwire, gpuwire, storagewire, wire as transportwire},
  },
  hypervisor::{
    gic, ram,
    state::group::{wire, Checkpoint},
  },
  snapshot::{
    manifest::{Binding, Record},
    package::{Package, Sealed, Stage},
    wire as softwarewire,
  },
};
use std::fs::File;

/// Serialize the immutable COW cut after releasing its brief VM pause.
/// The original operation and runtime owners must remain retained by the caller.
/// `acquire` fences the original policy across atomic publication; see `Stage::publish`.
pub fn save<G>(
  saved: &Checkpoint,
  parent: &File,
  binding: Binding,
  mut authorize: impl FnMut() -> anyhow::Result<()>,
  acquire: impl FnOnce() -> anyhow::Result<G>,
) -> anyhow::Result<Package> {
  encode(saved, parent, binding, &mut authorize)?.publish(authorize, acquire)
}

/// Serialize for a separate host-owned commit. No complete package is published here.
pub fn stage(
  saved: &Checkpoint,
  parent: &File,
  binding: Binding,
  mut authorize: impl FnMut() -> anyhow::Result<()>,
) -> anyhow::Result<Sealed> {
  encode(saved, parent, binding, &mut authorize)?.seal(authorize)
}

fn encode(
  saved: &Checkpoint,
  parent: &File,
  binding: Binding,
  mut authorize: impl FnMut() -> anyhow::Result<()>,
) -> anyhow::Result<Stage> {
  authorize()?;
  let mut stage = Stage::new(parent, binding, layout(saved)?, &mut authorize)?;
  for (index, transport) in saved.primary.devices.transports.iter().enumerate() {
    if let Some(transport) = transport {
      if let Some(bytes) = backendwire::encode(transport)? {
        write(
          &mut stage,
          Record::Backend(index as u8),
          &bytes,
          &mut authorize,
        )?;
      }
      if gpuwire::present(transport) {
        stage.write(
          Record::Backend(index as u8),
          &mut authorize,
          |mut output| gpuwire::encode(transport, &mut output, || Ok(())),
        )?;
      }
      if storagewire::present(transport) {
        stage.image(index as u8, &mut authorize, |directory, authorize| {
          storagewire::save(transport, directory, authorize)
        })?;
      }
      write(
        &mut stage,
        Record::Transport(index as u8),
        &transportwire::encode(transport)?,
        &mut authorize,
      )?;
    }
  }
  write(
    &mut stage,
    Record::Uart,
    &serial::wire::encode(&saved.primary.devices.console)?,
    &mut authorize,
  )?;
  write(
    &mut stage,
    Record::Rtc,
    &rtc::wire::encode(&saved.primary.devices.rtc),
    &mut authorize,
  )?;
  write(
    &mut stage,
    Record::Flash,
    &flash::snapshot::wire::encode(&saved.primary.devices.flash)?,
    &mut authorize,
  )?;
  write(
    &mut stage,
    Record::Software,
    &softwarewire::encode(&saved.primary.software, &saved.primary.actions)?,
    &mut authorize,
  )?;
  for (index, cpu) in saved.cpus.iter().enumerate() {
    write(
      &mut stage,
      Record::Cpu(index as u8),
      &wire::encode(cpu)?,
      &mut authorize,
    )?;
  }
  write(
    &mut stage,
    Record::Interrupts,
    &gic::state::wire::encode(&saved.primary.interrupts),
    &mut authorize,
  )?;
  for (slot, bank) in [
    &saved.primary.memory.firmware,
    &saved.primary.memory.nvram,
    &saved.primary.memory.ram,
  ]
  .into_iter()
  .enumerate()
  {
    stage.write(Record::Memory(slot as u8), &mut authorize, |mut output| {
      ram::wire::encode(bank, &mut output, || Ok(()))
    })?;
  }
  Ok(stage)
}

fn write(
  stage: &mut Stage,
  record: Record,
  bytes: &[u8],
  authorize: &mut impl FnMut() -> anyhow::Result<()>,
) -> anyhow::Result<()> {
  stage.write(record, authorize, |output| Ok(output.write_all(bytes)?))
}
