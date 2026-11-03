// private diagnostic files; production publication still requires pinned directory ownership.
use anyhow::ensure;
use std::{
  fs::{File, OpenOptions},
  os::unix::fs::{MetadataExt, OpenOptionsExt},
  path::Path,
};

fn private(root: &Path) -> anyhow::Result<()> {
  let metadata = std::fs::symlink_metadata(root)?;
  ensure!(
    metadata.is_dir()
      && metadata.uid() == unsafe { libc::geteuid() }
      && metadata.mode() & 0o777 == 0o700,
    "Diagnostic record directory is not private"
  );
  Ok(())
}

pub fn sync(root: &Path, file: &File) -> anyhow::Result<()> {
  file.sync_all()?;
  File::open(root)?.sync_all()?;
  Ok(())
}

pub fn create(root: &Path, name: &str) -> anyhow::Result<File> {
  private(root)?;
  Ok(
    OpenOptions::new()
      .write(true)
      .create_new(true)
      .mode(0o600)
      .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
      .open(root.join(name))?,
  )
}

pub fn reader(root: &Path, name: &str, limit: u64) -> anyhow::Result<File> {
  private(root)?;
  let file = OpenOptions::new()
    .read(true)
    .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
    .open(root.join(name))?;
  let metadata = file.metadata()?;
  ensure!(
    metadata.is_file()
      && metadata.mode() & 0o777 == 0o600
      && metadata.uid() == unsafe { libc::geteuid() }
      && metadata.nlink() == 1
      && metadata.len() <= limit,
    "Diagnostic record is not a private bounded file"
  );
  Ok(file)
}
