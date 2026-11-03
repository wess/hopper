use anyhow::{ensure, Context};
use machine::{
  devices::virtio::pci,
  runtime::variables,
  snapshot::{
    generation::{Generation, Stage},
    package::Package,
  },
};
use sha2::{Digest, Sha256};
use std::{
  fs::{self, File, OpenOptions, Permissions},
  os::unix::fs::{symlink, PermissionsExt},
  path::Path,
};

pub fn rejected(
  parent: &File,
  package: &Package,
  hardware: &[Option<pci::Device>; 7],
) -> anyhow::Result<()> {
  let error = Generation::open(parent, package, hardware, super::generation::ID, || Ok(()))
    .err()
    .context("Attempted generation unexpectedly admitted")?;
  ensure!(
    error.to_string().contains("attempted or requires recovery"),
    "Wrong replay rejection: {error}"
  );
  Ok(())
}

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
  for fault in 0..5 {
    let path = root.join(format!("attempt{fault}"));
    fs::create_dir(&path)?;
    fs::set_permissions(&path, Permissions::from_mode(0o700))?;
    let parent = File::open(&path)?;
    let ready =
      Stage::prepare(&parent, package, hardware, id, || Ok(()))?.publish(|| Ok(()), || Ok(()))?;
    let generation = path.join(&destination);
    let marker = generation.join("attempt.hga");
    if fault == 0 {
      ensure!(ready
        .consume::<()>(|| Ok(()), || anyhow::bail!("guard denied"))
        .is_err());
      ensure!(!marker.exists());
      let ready = Generation::open(&parent, package, hardware, id, || Ok(()))?;
      let observe = marker.clone();
      ensure!(ready
        .consume(
          move || {
            ensure!(!observe.exists(), "cancelled after durable attempt");
            Ok(())
          },
          || Ok(())
        )
        .is_err());
      let bytes = fs::read(&marker)?;
      ensure!(bytes.len() == 84 && &bytes[..4] == b"HGA1");
      ensure!(bytes[4..36] == Sha256::digest(fs::read(generation.join("generation.hgr"))?)[..]);
      ensure!(bytes[52..] == Sha256::digest(&bytes[..52])[..]);
      let disk = OpenOptions::new()
        .read(true)
        .write(true)
        .open(generation.join("storage0/disk"))?;
      fs2::FileExt::try_lock_exclusive(&disk)?;
      fs2::FileExt::unlock(&disk)?;
      drop(variables::open(&generation.join("variables"))?);
    } else {
      drop(ready);
      match fault {
        1 => fs::write(&marker, b"HGA")?,
        2 => symlink(path.join("missing"), &marker)?,
        3 => fs::create_dir(&marker)?,
        4 => {
          use std::{ffi::CString, os::unix::ffi::OsStrExt};
          let name = CString::new(marker.as_os_str().as_bytes())?;
          ensure!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) } == 0);
        }
        _ => unreachable!(),
      }
    }
    rejected(&parent, package, hardware)?;
    ensure!(
      fs::symlink_metadata(&marker).is_ok(),
      "Replay rejection removed attempt state"
    );
  }
  println!("Durable restore attempt rejects replay after cancelled handoff, preserves partial/symlink/directory/FIFO markers, leaves no marker on denied guard and releases disk/EFI owners");
  Ok(())
}
