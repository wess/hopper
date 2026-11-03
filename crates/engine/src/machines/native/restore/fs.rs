use anyhow::ensure;
use std::{
  ffi::CStr,
  fs::File,
  os::{
    fd::{AsRawFd, FromRawFd},
    unix::fs::MetadataExt,
  },
};

pub(super) fn private(file: &File) -> anyhow::Result<()> {
  let m = file.metadata()?;
  ensure!(
    m.is_dir() && m.uid() == unsafe { libc::geteuid() } && m.mode() & 0o7777 == 0o700,
    "Native restore directory must be private"
  );
  Ok(())
}

pub(super) fn directory(parent: &File, name: &CStr) -> anyhow::Result<File> {
  let fd = unsafe {
    libc::openat(
      parent.as_raw_fd(),
      name.as_ptr(),
      libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
    )
  };
  if fd < 0 {
    return Err(std::io::Error::last_os_error().into());
  }
  let file = unsafe { File::from_raw_fd(fd) };
  private(&file)?;
  Ok(file)
}

pub(super) fn file(parent: &File, name: &CStr, mode: u32, create: bool) -> anyhow::Result<File> {
  private(parent)?;
  let flags = if create {
    libc::O_RDWR | libc::O_CREAT | libc::O_EXCL
  } else if mode == 0o600 {
    libc::O_RDWR
  } else {
    libc::O_RDONLY
  };
  let fd = unsafe {
    libc::openat(
      parent.as_raw_fd(),
      name.as_ptr(),
      flags | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
      mode,
    )
  };
  if fd < 0 {
    return Err(std::io::Error::last_os_error().into());
  }
  let file = unsafe { File::from_raw_fd(fd) };
  let m = file.metadata()?;
  ensure!(
    m.is_file()
      && m.uid() == unsafe { libc::geteuid() }
      && m.nlink() == 1
      && m.mode() & 0o7777 == mode,
    "Native restore file must be private and regular"
  );
  Ok(file)
}

pub(super) fn seal(file: &File) -> anyhow::Result<()> {
  ensure!(
    unsafe { libc::fchmod(file.as_raw_fd(), 0o400) } == 0,
    "Seal native restore record: {}",
    std::io::Error::last_os_error()
  );
  file.sync_all()?;
  Ok(())
}

pub(super) fn require_absent(parent: &File, name: &CStr) -> anyhow::Result<()> {
  let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
  if unsafe {
    libc::fstatat(
      parent.as_raw_fd(),
      name.as_ptr(),
      metadata.as_mut_ptr(),
      libc::AT_SYMLINK_NOFOLLOW,
    )
  } == 0
  {
    anyhow::bail!("Selected restore generation was already attempted; recovery is required");
  }
  let error = std::io::Error::last_os_error();
  ensure!(
    error.raw_os_error() == Some(libc::ENOENT),
    "Inspect selected restore generation: {error}"
  );
  Ok(())
}
