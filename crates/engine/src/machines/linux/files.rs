use anyhow::{ensure, Context};
use std::{
  fs::{File, OpenOptions},
  os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
  path::Path,
};

pub(super) fn directory(path: &Path) -> anyhow::Result<()> {
  match std::fs::symlink_metadata(path) {
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
      match std::fs::DirBuilder::new().mode(0o700).create(path) {
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        result => result?,
      }
    }
    Err(error) => return Err(error.into()),
    Ok(_) => {}
  }
  let info = std::fs::symlink_metadata(path)?;
  ensure!(
    info.is_dir() && info.uid() == unsafe { libc::geteuid() } && info.mode() & 0o077 == 0,
    "Linux media directory must be private and owned"
  );
  Ok(())
}

pub(super) fn open(path: &Path, write: bool) -> anyhow::Result<File> {
  let file = OpenOptions::new()
    .read(true)
    .write(write)
    .create(write)
    .truncate(false)
    .mode(0o600)
    .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
    .open(path)?;
  let info = file.metadata()?;
  ensure!(
    info.is_file() && info.uid() == unsafe { libc::geteuid() } && info.mode() & 0o077 == 0,
    "Linux media file must be private and owned"
  );
  Ok(file)
}

pub(super) fn publish(partial: &Path, complete: &Path) -> anyhow::Result<()> {
  std::fs::hard_link(partial, complete)?;
  File::open(
    complete
      .parent()
      .context("Linux media needs a cache directory")?,
  )?
  .sync_all()?;
  std::fs::remove_file(partial)?;
  File::open(
    complete
      .parent()
      .context("Linux media needs a cache directory")?,
  )?
  .sync_all()?;
  Ok(())
}
