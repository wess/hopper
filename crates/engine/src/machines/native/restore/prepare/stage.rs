use super::{fs, Owner, Scope};
use crate::machines::native::restore::{Phase, Record};
use anyhow::{ensure, Context};
use machine::{
  runtime::hardware,
  snapshot::{
    checkpoint,
    generation::Stage,
    manifest::{Binding, Layout},
    package::Package,
  },
};
use model::native::{Boot, Restore};
use std::os::{fd::AsRawFd, unix::fs::MetadataExt};

pub(super) fn prepare(
  owner: &mut Owner,
  boot: Boot,
  snapshot: [u8; 16],
  rollback: [u8; 16],
  expected: Option<Record>,
  scope: Scope,
  configuration: Option<[u8; 32]>,
) -> anyhow::Result<(Boot, Restore)> {
  scope.check()?;
  let snapshots = fs::directory(&scope.root, c"snapshots")?;
  let inspected = configuration
    .map(|_| {
      Package::inspect(
        &snapshots,
        model::native::machine_identity(&boot.machine_id)
          .context("Invalid recovery VM identity")?,
        snapshot,
        || scope.check(),
      )
    })
    .transpose()?;
  let (_, hardware, configuration) = match (&inspected, configuration) {
    (Some(package), Some(configuration)) => {
      hardware::checkpoint(boot.clone(), package, configuration, || scope.check())?
    }
    _ => hardware::original(boot.clone(), || scope.check())?,
  };
  ensure!(
    expected.is_none_or(|record| record.configuration == configuration),
    "Restore predecessor hardware configuration differs"
  );
  let layout = Layout {
    cpus: boot.cpus.try_into()?,
    banks: [
      (0, 0x4000000),
      (0x4000000, 0x4000000),
      (machine::platform::RAM, boot.memory),
    ],
    devices: hardware.each_ref().map(|device| {
      device
        .as_ref()
        .map_or(0, machine::devices::virtio::pci::snapshot::records::kind)
    }),
  };
  let binding = Binding {
    machine: model::native::machine_identity(&boot.machine_id)
      .context("Invalid restore VM identity")?,
    snapshot,
    configuration,
  };
  let source = match inspected {
    Some(package) => package,
    None => Package::open(&snapshots, binding, layout, || scope.check())?,
  };
  let prior = Package::open(
    &snapshots,
    Binding {
      snapshot: rollback,
      ..binding
    },
    layout,
    || scope.check(),
  )?;
  drop(checkpoint::load(&prior, &hardware, || scope.check())?);
  scope.check()?;
  let result = unsafe { libc::mkdirat(scope.root.as_raw_fd(), c"generations".as_ptr(), 0o700) };
  ensure!(
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EEXIST),
    "Create restore generations: {}",
    std::io::Error::last_os_error()
  );
  let generations = fs::directory(&scope.root, c"generations")?;
  let check = || {
    scope.check()?;
    let observed = fs::directory(&scope.root, c"generations")?.metadata()?;
    let expected = generations.metadata()?;
    ensure!(
      (observed.dev(), observed.ino()) == (expected.dev(), expected.ino()),
      "Restore generation namespace changed"
    );
    Ok(())
  };
  let generation = *uuid::Uuid::new_v4().as_bytes();
  let stage = Stage::prepare(&generations, &source, &hardware, generation, check)?;
  let ready = stage.publish(check, || owner.commit())?;
  let selected = Record {
    machine: binding.machine,
    snapshot,
    generation,
    configuration,
    rollback,
    previous: expected.map(|record| record.generation),
    phase: Phase::Prepared,
  };
  check()?;
  if let Some(expected) = expected {
    owner.succeed(expected, selected, layout, check)?;
  } else {
    owner.select(selected, layout, check)?;
  }
  owner.attempted(selected, check)?;
  check()?;
  let policy_generation = owner.policy_generation()?;
  drop(ready);
  drop(hardware);
  Ok((
    boot,
    Restore {
      snapshot_id: uuid::Uuid::from_bytes(snapshot).to_string(),
      generation_id: uuid::Uuid::from_bytes(generation).to_string(),
      policy_generation,
    },
  ))
}
