use crate::machines::Machines;
use anyhow::ensure;
use std::{
  fs::{Metadata, OpenOptions},
  os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
  path::{Path, PathBuf},
};

pub(super) fn uid() -> u32 {
  unsafe { libc::geteuid() }
}

fn private(info: &Metadata) -> anyhow::Result<()> {
  ensure!(
    info.uid() == uid() && info.permissions().mode() & 0o077 == 0,
    "Native agent endpoint must be private and owned by this user"
  );
  Ok(())
}

pub(super) fn endpoint(manager: &Machines) -> anyhow::Result<PathBuf> {
  let folder = manager.root.join("agents");
  let info = std::fs::symlink_metadata(&folder)?;
  ensure!(info.is_dir(), "Native agent directory cannot be a symlink");
  private(&info)?;
  Ok(folder.join("native.sock"))
}

pub(super) fn socket(path: &Path) -> anyhow::Result<Metadata> {
  let info = std::fs::symlink_metadata(path)?;
  ensure!(
    info.file_type().is_socket(),
    "Native agent endpoint is not a socket"
  );
  private(&info)?;
  Ok(info)
}

pub(super) fn prepare(manager: &Machines) -> anyhow::Result<(PathBuf, store::lock::Lease)> {
  std::fs::create_dir_all(&manager.root)?;
  let folder = manager.root.join("agents");
  match std::fs::DirBuilder::new().mode(0o700).create(&folder) {
    Ok(()) => {}
    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
    Err(error) => return Err(error.into()),
  }
  let endpoint = endpoint(manager)?;
  let file = OpenOptions::new()
    .read(true)
    .write(true)
    .create(true)
    .truncate(false)
    .mode(0o600)
    .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
    .open(folder.join("owner"))?;
  let info = file.metadata()?;
  private(&info)?;
  ensure!(
    info.is_file() && info.nlink() == 1,
    "Invalid native agent ownership file"
  );
  let lease = store::lock::exclusive(file)?;
  match std::fs::symlink_metadata(&endpoint) {
    Ok(_) => {
      socket(&endpoint)?;
      std::fs::remove_file(&endpoint)?;
    }
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
    Err(error) => return Err(error.into()),
  }
  Ok((endpoint, lease))
}

pub(super) struct Socket {
  pub path: PathBuf,
  pub identity: Metadata,
}

impl Drop for Socket {
  fn drop(&mut self) {
    if std::fs::symlink_metadata(&self.path).is_ok_and(|info| {
      info.file_type().is_socket()
        && info.dev() == self.identity.dev()
        && info.ino() == self.identity.ino()
    }) {
      let _ = std::fs::remove_file(&self.path);
    }
  }
}
