use crate::machines::native::assets;
use anyhow::ensure;
use machine::vz::queue::Check;
use std::{
  ffi::CString,
  fs::File,
  io::{Read, Write},
  os::{
    fd::AsRawFd,
    unix::fs::{MetadataExt, PermissionsExt},
  },
  path::Path,
};

pub fn stage(path: &Path, parent: &Path, check: &Check) -> anyhow::Result<tempfile::TempDir> {
  check()?;
  ensure!(
    path.is_absolute() && parent.is_absolute(),
    "Restore paths must be absolute"
  );
  super::super::files::directory(parent)?;
  let mut source = assets::regular(path, 64 << 30)?;
  let original = source.metadata()?;
  ensure!(
    (1..=64 << 30).contains(&original.len()),
    "Restore image size exceeds bounds"
  );
  let directory = tempfile::Builder::new()
    .prefix("restore")
    .tempdir_in(parent)?;
  std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
  let destination = directory.path().join("restore.ipsw");
  let folder = File::open(directory.path())?;
  let name = CString::new("restore.ipsw")?;
  check()?;
  if unsafe { libc::fclonefileat(source.as_raw_fd(), folder.as_raw_fd(), name.as_ptr(), 0) } != 0 {
    let error = std::io::Error::last_os_error();
    ensure!(
      matches!(
        error.raw_os_error(),
        Some(libc::EXDEV | libc::ENOTSUP | libc::EINVAL)
      ),
      "Clone macOS restore image: {error}"
    );
    ensure!(
      fs2::available_space(directory.path())? >= original.len() + (64 << 20),
      "Staging macOS restore media needs more free disk space"
    );
    let mut output = super::super::files::write(&destination)?;
    let mut chunk = vec![0; 1 << 20];
    let mut copied = 0;
    loop {
      check()?;
      let count = source.read(&mut chunk)?;
      if count == 0 {
        break;
      }
      copied += count as u64;
      ensure!(
        copied <= original.len(),
        "Restore image changed while staging"
      );
      output.write_all(&chunk[..count])?;
    }
    ensure!(
      copied == original.len(),
      "Restore image changed while staging"
    );
    output.sync_all()?;
  }
  std::fs::set_permissions(&destination, std::fs::Permissions::from_mode(0o600))?;
  let output = super::super::files::read(&destination, 64 << 30)?;
  ensure!(
    output.metadata()?.len() == original.len(),
    "Staged restore image size changed"
  );
  output.sync_all()?;
  folder.sync_all()?;
  let current = source.metadata()?;
  ensure!(
    (
      current.dev(),
      current.ino(),
      current.len(),
      current.mtime(),
      current.mtime_nsec(),
      current.ctime(),
      current.ctime_nsec()
    ) == (
      original.dev(),
      original.ino(),
      original.len(),
      original.mtime(),
      original.mtime_nsec(),
      original.ctime(),
      original.ctime_nsec()
    ),
    "Restore image changed while staging"
  );
  check()?;
  Ok(directory)
}
