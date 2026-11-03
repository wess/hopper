//! private ephemeral APFS clones; durable snapshot publication is separate.

use anyhow::{ensure, Context};
use std::{
  fs::{File, OpenOptions, Permissions},
  os::{
    fd::{AsRawFd, FromRawFd},
    unix::fs::{FileExt, MetadataExt, OpenOptionsExt, PermissionsExt},
  },
};
use tempfile::TempDir;
mod checksum;
mod current;
mod extent;
pub use current::Current;
mod prepare;
pub mod record;
pub use prepare::Prepared;

pub struct Image {
  file: File,
  _directory: TempDir,
  size: u64,
}

pub struct Restored {
  pub(crate) file: File,
  pub(crate) directory: TempDir,
}

extern "C" {
  fn fclonefileat(source: i32, directory: i32, name: *const libc::c_char, flags: u32) -> i32;
}

/// The caller holds every writer and retains the source's runtime/operation ownership.
/// Unsupported or cross-volume cloning rejects; this never falls back to a full copy.
pub fn capture(source: &File) -> anyhow::Result<Image> {
  let (file, directory) = clone(source, true)?;
  let size = file.metadata()?.len();
  Ok(Image {
    file,
    _directory: directory,
    size,
  })
}

impl Image {
  pub fn size(&self) -> u64 {
    self.size
  }

  pub fn read(&self, offset: u64, bytes: &mut [u8]) -> anyhow::Result<()> {
    ensure!(bytes.len() <= 65536, "Captured disk reads exceed 64 KiB");
    ensure!(
      offset
        .checked_add(bytes.len() as u64)
        .is_some_and(|end| end <= self.size),
      "Read exceeds captured disk"
    );
    self
      .file
      .read_exact_at(bytes, offset)
      .context("Read captured disk")
  }

  pub fn fork_readonly(&self) -> anyhow::Result<Restored> {
    let (file, directory) = clone(&self.file, true)?;
    Ok(Restored { file, directory })
  }

  pub fn fork(&self) -> anyhow::Result<Restored> {
    let (file, directory) = clone(&self.file, false)?;
    Ok(Restored { file, directory })
  }
}

impl Restored {
  pub fn file(&self) -> &File {
    &self.file
  }
}

fn clone(source: &File, readonly: bool) -> anyhow::Result<(File, TempDir)> {
  let before = source.metadata()?;
  ensure!(
    before.is_file() && (1..=2 * 1024u64.pow(4)).contains(&before.len()),
    "Snapshot source must be a regular file of at most 2 TiB"
  );
  let directory = tempfile::Builder::new()
    .prefix("hoppersnapshot")
    .permissions(Permissions::from_mode(0o700))
    .tempdir()?;
  let parent = OpenOptions::new()
    .read(true)
    .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
    .open(directory.path())?;
  let metadata = parent.metadata()?;
  ensure!(
    metadata.is_dir()
      && metadata.uid() == unsafe { libc::geteuid() }
      && metadata.mode() & 0o777 == 0o700,
    "Snapshot directory is not private"
  );
  if unsafe { fclonefileat(source.as_raw_fd(), parent.as_raw_fd(), c"image".as_ptr(), 3) } != 0 {
    return Err(std::io::Error::last_os_error()).context("Clone snapshot image on APFS");
  }
  let fd = unsafe {
    libc::openat(
      parent.as_raw_fd(),
      c"image".as_ptr(),
      libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
    )
  };
  ensure!(
    fd >= 0,
    "Open cloned snapshot image: {}",
    std::io::Error::last_os_error()
  );
  let cloned = unsafe { File::from_raw_fd(fd) };
  cloned.set_permissions(Permissions::from_mode(if readonly { 0o400 } else { 0o600 }))?;
  let copied = cloned.metadata()?;
  ensure!(
    copied.is_file()
      && copied.len() == before.len()
      && copied.nlink() == 1
      && copied.uid() == unsafe { libc::geteuid() }
      && (copied.dev(), copied.ino()) != (before.dev(), before.ino()),
    "Cloned snapshot image identity does not match"
  );
  cloned.sync_all()?;
  parent.sync_all()?;
  if readonly {
    return Ok((cloned, directory));
  }
  let fd = unsafe {
    libc::openat(
      parent.as_raw_fd(),
      c"image".as_ptr(),
      libc::O_RDWR | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
    )
  };
  ensure!(
    fd >= 0,
    "Open writable snapshot image: {}",
    std::io::Error::last_os_error()
  );
  let file = unsafe { File::from_raw_fd(fd) };
  let reopened = file.metadata()?;
  ensure!(
    (reopened.dev(), reopened.ino()) == (copied.dev(), copied.ino())
      && reopened.len() == copied.len(),
    "Snapshot image changed during admission"
  );
  Ok((file, directory))
}
