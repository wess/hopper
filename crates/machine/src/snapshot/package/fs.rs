use anyhow::{ensure, Context};
use std::{
  ffi::{CStr, CString},
  fs::File,
  os::{
    fd::{AsRawFd, FromRawFd},
    unix::fs::MetadataExt,
  },
};

pub(crate) fn name(value: &str) -> CString {
  CString::new(value).expect("internally derived snapshot name")
}

pub(crate) fn private(directory: &File) -> anyhow::Result<()> {
  let metadata = directory.metadata()?;
  ensure!(
    metadata.is_dir()
      && metadata.uid() == unsafe { libc::geteuid() }
      && metadata.mode() & 0o7777 == 0o700,
    "Snapshot directory is not private"
  );
  Ok(())
}

pub(crate) fn directory(parent: &File, name: &CStr) -> anyhow::Result<File> {
  let fd = unsafe {
    libc::openat(
      parent.as_raw_fd(),
      name.as_ptr(),
      libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
    )
  };
  ensure!(
    fd >= 0,
    "Open snapshot directory: {}",
    std::io::Error::last_os_error()
  );
  let file = unsafe { File::from_raw_fd(fd) };
  private(&file)?;
  Ok(file)
}

pub(crate) fn mkdir(parent: &File, name: &CStr) -> anyhow::Result<File> {
  ensure!(
    unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) } == 0,
    "Create snapshot directory: {}",
    std::io::Error::last_os_error()
  );
  directory(parent, name)
}

pub(crate) fn create(parent: &File, name: &CStr) -> anyhow::Result<File> {
  let fd = unsafe {
    libc::openat(
      parent.as_raw_fd(),
      name.as_ptr(),
      libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
      0o600,
    )
  };
  ensure!(
    fd >= 0,
    "Create snapshot component: {}",
    std::io::Error::last_os_error()
  );
  Ok(unsafe { File::from_raw_fd(fd) })
}

pub(crate) fn open(parent: &File, name: &CStr, limit: u64) -> anyhow::Result<File> {
  let fd = unsafe {
    libc::openat(
      parent.as_raw_fd(),
      name.as_ptr(),
      libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
    )
  };
  ensure!(
    fd >= 0,
    "Open snapshot component: {}",
    std::io::Error::last_os_error()
  );
  let file = unsafe { File::from_raw_fd(fd) };
  let metadata = file.metadata()?;
  ensure!(
    metadata.is_file()
      && metadata.uid() == unsafe { libc::geteuid() }
      && metadata.mode() & 0o7777 == 0o400
      && metadata.nlink() == 1
      && metadata.len() <= limit,
    "Snapshot component is not private and bounded"
  );
  Ok(file)
}

pub(crate) fn seal(file: &File) -> anyhow::Result<()> {
  ensure!(
    unsafe { libc::fchmod(file.as_raw_fd(), 0o400) } == 0,
    "Seal snapshot component: {}",
    std::io::Error::last_os_error()
  );
  file.sync_all().context("Sync snapshot component")
}

pub(crate) fn same(parent: &File, name: &CStr, expected: &File) -> anyhow::Result<()> {
  let opened = directory(parent, name)?.metadata()?;
  let pinned = expected.metadata()?;
  ensure!(
    (opened.dev(), opened.ino()) == (pinned.dev(), pinned.ino()),
    "Snapshot stage directory was replaced"
  );
  Ok(())
}
