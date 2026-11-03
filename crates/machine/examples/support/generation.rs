use anyhow::{ensure, Context};
use machine::{
  devices::virtio::pci,
  hypervisor::state::group::Checkpoint,
  runtime::{recovery::Disks, variables::Variables},
  snapshot::{
    checkpoint,
    generation::{Generation, Stage},
  },
};
use std::{
  fs::{File, Permissions},
  os::unix::fs::PermissionsExt,
  path::Path,
};

pub const ID: [u8; 16] = [4; 16];

pub fn resources(
  root: &Path,
  saved: &Checkpoint,
  faults: bool,
  devices: &impl Fn() -> anyhow::Result<[Option<pci::Device>; 7]>,
) -> anyhow::Result<(Checkpoint, Variables, Disks)> {
  std::fs::set_permissions(root, Permissions::from_mode(0o700))?;
  for child in ["snapshots", "generations"] {
    std::fs::create_dir(root.join(child))?;
    std::fs::set_permissions(root.join(child), Permissions::from_mode(0o700))?;
  }
  let snapshots = File::open(root.join("snapshots"))?;
  let parent = File::open(root.join("generations"))?;
  let package = checkpoint::save(
    saved,
    &snapshots,
    super::records::binding(),
    || Ok(()),
    || Ok(()),
  )?;
  let hardware = devices()?;
  if faults {
    super::generationfailures::run(root, &package, &hardware)?;
    super::generationattempts::run(root, &package, &hardware)?;
  }
  let stage = Stage::prepare(&parent, &package, &hardware, ID, || Ok(()))?;
  let ready = stage.publish(|| Ok(()), || Ok(()))?;
  // reopening must not steal the original disk/EFI owners retained through publication.
  ensure!(Generation::open(&parent, &package, &hardware, ID, || Ok(())).is_err());
  drop(ready);
  let ready = Generation::open(&parent, &package, &hardware, ID, || Ok(()))?;
  let result = ready.consume(|| Ok(()), || Ok(()))?;
  super::generationattempts::rejected(&parent, &package, &hardware)?;
  ensure!(disk(root)
    .parent()
    .and_then(|path| path.parent())
    .context("Generation directory")?
    .join("attempt.hga")
    .is_file());
  println!("Bound restore generation published, original owners dropped, checkpoint/disk/EFI reopened together and admitted before hardware execution");
  Ok(result)
}

pub fn disk(root: &Path) -> std::path::PathBuf {
  root
    .join("generations")
    .join(
      ID.iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>(),
    )
    .join("storage0/disk")
}

pub fn replay(
  root: &Path,
  saved: &Checkpoint,
  devices: &impl Fn() -> anyhow::Result<[Option<pci::Device>; 7]>,
  boot: &impl Fn() -> machine::runtime::Boot,
  expected: u8,
) -> anyhow::Result<()> {
  let parent = File::open(root.join("generations"))?;
  let snapshots = File::open(root.join("snapshots"))?;
  let package = machine::snapshot::package::Package::open(
    &snapshots,
    super::records::binding(),
    checkpoint::layout(saved)?,
    || Ok(()),
  )?;
  super::generationattempts::rejected(&parent, &package, &devices()?)?;
  println!("Consumed restore generation rejects replay after complete runtime/device/EFI teardown");
  super::current::run(root, boot, devices, expected)?;
  Ok(())
}
