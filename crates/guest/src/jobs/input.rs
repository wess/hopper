use anyhow::ensure;
use std::{
  fs::File,
  io::{Seek, SeekFrom, Write},
  os::fd::{AsRawFd, FromRawFd},
};

pub(super) fn sealed(bytes: &[u8]) -> anyhow::Result<File> {
  ensure!(
    bytes.len() <= model::guest::files::CREATE_LIMIT,
    "Guest file payload exceeds bounds"
  );
  let fd = unsafe {
    libc::memfd_create(
      c"hopper.file".as_ptr(),
      libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING,
    )
  };
  ensure!(
    fd >= 0,
    "Create guest file payload: {}",
    std::io::Error::last_os_error()
  );
  let mut file = unsafe { File::from_raw_fd(fd) };
  ensure!(
    unsafe { libc::fchmod(file.as_raw_fd(), 0o400) } == 0,
    "Make guest file payload private: {}",
    std::io::Error::last_os_error()
  );
  file.write_all(bytes)?;
  file.seek(SeekFrom::Start(0))?;
  let seals = libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL;
  ensure!(
    unsafe { libc::fcntl(file.as_raw_fd(), libc::F_ADD_SEALS, seals) } == 0,
    "Seal guest file payload: {}",
    std::io::Error::last_os_error()
  );
  Ok(file)
}
