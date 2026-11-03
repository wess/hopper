use anyhow::{ensure, Context};
use machine::{
  devices::virtio::pci,
  snapshot::{
    generation::{Generation, Stage},
    package::Package,
  },
};
use std::{
  fs::{self, File, OpenOptions, Permissions},
  os::unix::fs::{FileExt, PermissionsExt},
  path::Path,
};

pub fn run(
  root: &Path,
  package: &Package,
  hardware: &[Option<pci::Device>; 7],
) -> anyhow::Result<()> {
  let id = super::generation::ID;
  let destination = id
    .iter()
    .map(|byte| format!("{byte:02x}"))
    .collect::<String>();
  for fault in 0..7 {
    let path = root.join(format!("fault{fault}"));
    fs::create_dir(&path)?;
    fs::set_permissions(&path, Permissions::from_mode(0o700))?;
    let parent = File::open(&path)?;
    if fault == 0 {
      ensure!(Stage::prepare(&parent, package, hardware, id, || anyhow::bail!("revoked")).is_err());
      ensure!(fs::read_dir(&path)?.next().is_none());
      continue;
    }
    let stage = Stage::prepare(&parent, package, hardware, id, || Ok(()))?;
    let pending = fs::read_dir(&path)?
      .next()
      .context("Missing restore stage")??
      .path();
    if fault == 1 {
      let receipt = pending.join("generation.hgr");
      fs::set_permissions(&receipt, Permissions::from_mode(0o600))?;
      OpenOptions::new()
        .write(true)
        .open(&receipt)?
        .write_all_at(b"X", 0)?;
      fs::set_permissions(&receipt, Permissions::from_mode(0o400))?;
      ensure!(stage.publish(|| Ok(()), || Ok(())).is_err());
    } else if fault == 2 {
      let retained = path.join("retained");
      fs::rename(&pending, &retained)?;
      fs::create_dir(&pending)?;
      fs::set_permissions(&pending, Permissions::from_mode(0o700))?;
      ensure!(stage.publish(|| Ok(()), || Ok(())).is_err());
      ensure!(retained.join("generation.hgr").exists() && fs::read_dir(&pending)?.next().is_none());
    } else if fault == 3 {
      ensure!(stage
        .publish::<()>(|| Ok(()), || anyhow::bail!("policy guard denied"))
        .is_err());
    } else if fault == 4 {
      // cancellation after rename preserves a complete generation and releases its disk/EFI owners.
      ensure!(stage
        .publish(
          || {
            ensure!(!path.join(&destination).exists(), "cancelled after rename");
            Ok(())
          },
          || Ok(())
        )
        .is_err());
      drop(Generation::open(&parent, package, hardware, id, || Ok(()))?);
      continue;
    } else if fault == 6 {
      let variables = pending.join("variables");
      fs::rename(&variables, pending.join("retainedvariables"))?;
      fs::create_dir(&variables)?;
      fs::set_permissions(&variables, Permissions::from_mode(0o700))?;
      ensure!(stage.publish(|| Ok(()), || Ok(())).is_err());
      ensure!(pending.join("retainedvariables/bank").exists());
    } else {
      drop(stage.publish(|| Ok(()), || Ok(()))?);
      let original = fs::read(path.join(&destination).join("generation.hgr"))?;
      let duplicate = Stage::prepare(&parent, package, hardware, id, || Ok(()))?;
      ensure!(duplicate.publish(|| Ok(()), || Ok(())).is_err());
      ensure!(fs::read(path.join(&destination).join("generation.hgr"))? == original);
      drop(Generation::open(&parent, package, hardware, id, || Ok(()))?);
      continue;
    }
    ensure!(!path.join(&destination).exists() && pending.exists());
  }
  println!("Restore generation rejects revoked preparation, altered receipts, replaced stages/EFI children and denied guards; preserves completed rename on cancellation and refuses replacement");
  Ok(())
}
