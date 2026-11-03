use super::{checkpoint, optical, recapture, records, recovery, storage};
use anyhow::ensure;
use machine::{
  hypervisor::state::group::Checkpoint,
  runtime::Boot,
  snapshot::{
    checkpoint as durable,
    manifest::{Binding, Layout},
    package::Package,
  },
};
use std::{cell::Cell, fs::File};

pub enum Operation {
  Save(File),
  Load(File),
}

pub fn operation() -> anyhow::Result<Option<Operation>> {
  let arguments = std::env::args().skip(1).collect::<Vec<_>>();
  if arguments.is_empty() {
    return Ok(None);
  }
  ensure!(
    arguments.len() == 2,
    "Expected save/load with a private fixture directory"
  );
  let parent = File::open(&arguments[1])?;
  match arguments[0].as_str() {
    "save" => Ok(Some(Operation::Save(parent))),
    "load" => Ok(Some(Operation::Load(parent))),
    _ => anyhow::bail!("Expected save/load with a private fixture directory"),
  }
}

fn layout() -> Layout {
  Layout {
    cpus: 2,
    banks: [
      (0, 0x4000000),
      (0x4000000, 0x4000000),
      (machine::platform::RAM, 0x10000000),
    ],
    devices: [1, 2, 3, 4, 0, 0, 0],
  }
}

pub fn binding(
  boot: &Boot,
  hardware: &[Option<machine::devices::virtio::pci::Device>; 7],
  authorize: impl FnMut() -> anyhow::Result<()>,
) -> anyhow::Result<Binding> {
  let mut binding = records::binding();
  binding.configuration = machine::snapshot::configuration::fingerprint(
    "01010101-0101-0101-0101-010101010101",
    boot,
    hardware,
    authorize,
  )?;
  Ok(binding)
}

pub struct Writer {
  parent: File,
  binding: Binding,
  existed: bool,
  job: machine::snapshot::publication::Publication<File, machine::snapshot::package::Sealed>,
}

impl Writer {
  pub fn poll(&mut self) -> anyhow::Result<Option<Package>> {
    let Some(sealed) = self.job.poll()? else {
      return Ok(None);
    };
    ensure!(
      Package::open(&self.parent, self.binding, layout(), || self.job.check()).is_ok()
        == self.existed
    );
    let receipt = serde_json::from_slice(&serde_json::to_vec(&sealed.receipt())?)?;
    drop(sealed);
    let reopened = machine::snapshot::package::Sealed::open(
      &self.parent,
      receipt,
      self.binding,
      layout(),
      || self.job.check(),
    )?;
    let package = reopened.publish(|| self.job.check(), || Ok(()))?;
    println!(
      "Background stage committed by its diagnostic owner after original authorization recheck"
    );
    Ok(Some(package))
  }
}

pub fn publish(
  parent: &File,
  saved: Checkpoint,
  binding: Binding,
  resumed: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> anyhow::Result<Writer> {
  use std::{
    sync::{
      atomic::{AtomicUsize, Ordering},
      Arc,
    },
    time::{Duration, Instant},
  };
  ensure!(durable::layout(&saved)? == layout());
  let deny = std::env::var("HOPPER_VMSTATE_SAVE_DENY")
    .ok()
    .map(|value| value.parse::<usize>())
    .transpose()?;
  let calls = AtomicUsize::new(0);
  let check = Arc::new(move || {
    let current = calls.fetch_add(1, Ordering::AcqRel);
    ensure!(Some(current) != deny, "Snapshot authorization revoked");
    if current != 0 {
      // hold background work until actual guest progress proves the owner resumed.
      let deadline = Instant::now() + Duration::from_secs(1);
      while !resumed.load(Ordering::Acquire) {
        ensure!(
          Instant::now() < deadline,
          "Guest did not resume before background publication"
        );
        std::thread::sleep(Duration::from_millis(1));
      }
    }
    Ok(())
  });
  let output = parent.try_clone()?;
  let existed = Package::open(parent, binding, layout(), || Ok(())).is_ok();
  let job = machine::snapshot::publication::stage(
    saved,
    parent.try_clone()?,
    binding,
    parent.try_clone()?,
    Duration::from_secs(30),
    check,
  )?;
  Ok(Writer {
    parent: output,
    binding,
    existed,
    job,
  })
}

pub fn load(parent: &File, boot: &impl Fn() -> Boot) -> anyhow::Result<()> {
  let deny = std::env::var("HOPPER_VMSTATE_DENY")
    .ok()
    .map(|value| value.parse::<usize>())
    .transpose()?;
  let checks = Cell::new(0);
  let authorize = || {
    let current = checks.get();
    checks.set(current + 1);
    ensure!(Some(current) != deny, "Snapshot authorization revoked");
    Ok(())
  };
  let hardware = recovery::devices()?;
  let binding = binding(&boot(), &hardware, authorize)?;
  let package = Package::open(parent, binding, layout(), authorize)?;
  let mut saved = durable::load(&package, &hardware, authorize)?;
  println!("Checkpoint authorization checks: {}", checks.get());
  let mut bytes = [0; 16];
  saved.primary.memory.ram.read(0x10000, &mut bytes)?;
  let held = [
    u64::from_le_bytes(bytes[..8].try_into()?),
    u64::from_le_bytes(bytes[8..].try_into()?),
  ];
  checkpoint::verify(&saved, held)?;
  checkpoint::restore_device(&saved)?;
  storage::restore(&saved)?;
  optical::restore(&saved)?;
  recovery::run(&mut saved, boot)?;
  recapture::run(&saved, boot)?;
  println!("Fresh process restored the complete published checkpoint, resumed both CPUs and independently recaptured device/disk state");
  Ok(())
}
