use anyhow::{ensure, Context};
use machine::{
  devices::virtio::pci,
  hypervisor as hv,
  runtime::{self, variables, Boot, Control, Mode},
  snapshot::{
    generation::System,
    selection::{Phase, Record},
  },
};
use std::{
  fs::{File, OpenOptions},
  os::unix::fs::{FileExt, OpenOptionsExt},
  path::Path,
};

pub fn run(
  root: &Path,
  boot: &impl Fn() -> Boot,
  devices: &impl Fn() -> anyhow::Result<[Option<pci::Device>; 7]>,
  expected: u8,
) -> anyhow::Result<()> {
  let binding = super::records::binding();
  let record = Record {
    machine: binding.machine,
    generation: super::generation::ID,
    snapshot: binding.snapshot,
    configuration: binding.configuration,
    rollback: [8; 16],
    previous: None,
    phase: Phase::Active,
  };
  // this core fixture supplies synthetic host selection; real host admission is checked separately.
  let mut intent = OpenOptions::new()
    .write(true)
    .create_new(true)
    .mode(0o400)
    .open(root.join("restore.hrr"))?;
  std::io::Write::write_all(&mut intent, &record.encode()?)?;
  intent.sync_all()?;
  let parent = File::open(root)?;
  parent.sync_all()?;
  let system = System::open(&parent, record, || Ok(()))?;
  let layout = system.layout();
  let (mut store, mut disks) = system.admit(|| Ok(()))?;
  let mut hardware = devices()?;
  let mut config = boot();
  let id = "01010101-0101-0101-0101-010101010101";
  ensure!(model::native::machine_identity(id) == Some(binding.machine));
  let fingerprint =
    machine::snapshot::configuration::fingerprint(id, &config, &hardware, || Ok(()))?;
  ensure!(layout.devices[0] == 1 && disks.iter().skip(1).all(Option::is_none));
  hardware[0] = Some(pci::create(
    disks[0]
      .take()
      .context("Missing current disk")?
      .disk([b'd'; 20], || Ok(()))?,
  )?);
  ensure!(
    machine::snapshot::configuration::fingerprint(id, &config, &hardware, || Ok(()))?
      == fingerprint,
    "Current generation changed immutable hardware"
  );
  config.variables = variables::bytes(&store).to_vec();
  let mut phase = 0;
  let stopped = runtime::run_persistent(config, hardware, &mut store, |hardware, ram, mode| {
    if phase == 0 {
      ensure!(mode == Mode::Running);
      let mut bytes = [0; 16];
      hv::read(ram, 0x10000, &mut bytes)?;
      ensure!(bytes == [0; 16], "Cold startup replayed captured RAM");
      super::storage::fresh(
        hardware[0].as_mut().context("Current disk missing")?,
        ram,
        expected,
      )?;
      phase = 1;
      return Ok(Control::Continue);
    }
    let mut bytes = [0; 16];
    hv::read(ram, 0x10000, &mut bytes)?;
    if phase == 1
      && mode == Mode::Running
      && [0, 8].iter().all(|&offset| {
        u64::from_le_bytes(bytes[offset..offset + 8].try_into().expect("fixed counter")) > 0
      })
    {
      phase = 2;
      return Ok(Control::Pause);
    }
    if phase == 2 && mode == Mode::Paused {
      phase = 3;
      return Ok(Control::Stop);
    }
    Ok(Control::Continue)
  })?;
  ensure!(phase == 3 && stopped.reason == runtime::Reason::Stopped);
  let probe = OpenOptions::new()
    .read(true)
    .write(true)
    .open(super::generation::disk(root))?;
  ensure!(
    fs2::FileExt::try_lock_exclusive(&probe).is_err(),
    "Cold runtime released disk before devices"
  );
  drop(stopped);
  drop(store);
  fs2::FileExt::try_lock_exclusive(&probe)?;
  fs2::FileExt::unlock(&probe)?;
  let mut bytes = [0; 512];
  probe.read_exact_at(&mut bytes, 0)?;
  ensure!(
    bytes == [b'e'; 512],
    "Cold runtime did not retain new virtio writes"
  );
  drop(System::open(&parent, record, || Ok(()))?);
  println!("Active generation cold boot retains mutable disk/EFI, starts with fresh RAM and controller queues, executes both CPUs, persists new virtio writes and reopens after all owners exit");
  Ok(())
}
