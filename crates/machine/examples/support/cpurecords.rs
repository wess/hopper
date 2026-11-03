use machine::{
  devices::virtio::pci,
  hypervisor::state::group::Checkpoint,
  snapshot::{
    checkpoint,
    manifest::Binding,
    package::{Package, Sealed},
  },
};
use std::{
  fs::{File, Permissions},
  os::unix::fs::PermissionsExt,
};

pub fn binding() -> Binding {
  // synthetic identity is confined to this isolated diagnostic profile.
  Binding {
    machine: [1; 16],
    snapshot: [2; 16],
    configuration: [3; 32],
  }
}

pub fn reopen(
  saved: Checkpoint,
  hardware: impl FnOnce() -> anyhow::Result<[Option<pci::Device>; 7]>,
) -> anyhow::Result<Checkpoint> {
  let directory = tempfile::tempdir()?;
  std::fs::set_permissions(directory.path(), Permissions::from_mode(0o700))?;
  let parent = File::open(directory.path())?;
  let layout = checkpoint::layout(&saved)?;
  let sealed = checkpoint::stage(&saved, &parent, binding(), || Ok(()))?;
  let receipt = serde_json::from_slice(&serde_json::to_vec(&sealed.receipt())?)?;
  drop(sealed);
  drop(saved);
  anyhow::ensure!(Package::open(&parent, binding(), layout, || Ok(())).is_err());
  drop(
    Sealed::open(&parent, receipt, binding(), layout, || Ok(()))?.publish(|| Ok(()), || Ok(()))?,
  );
  let hardware = hardware()?;
  let package = Package::open(&parent, binding(), layout, || Ok(()))?;
  let reopened = checkpoint::load(&package, &hardware, || Ok(()))?;
  println!("Sealed checkpoint handoff committed and reopened after original checkpoint teardown; all consumed components verified against fresh devices");
  Ok(reopened)
}
