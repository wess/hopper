use anyhow::ensure;
use std::{fs::File, os::fd::AsRawFd};

pub(super) const CHUNK: u64 = 65536;

/// Filesystems may report zero-filled allocated regions as data; never infer holes from bytes.
pub(super) fn next(file: &File, offset: u64, size: u64) -> anyhow::Result<u64> {
  let offset = libc::off_t::try_from(offset)?;
  let fd = file.as_raw_fd();
  let position = unsafe { libc::lseek(fd, 0, libc::SEEK_CUR) };
  ensure!(
    position >= 0,
    "Read snapshot disk cursor: {}",
    std::io::Error::last_os_error()
  );
  let observed = unsafe { libc::lseek(fd, offset, libc::SEEK_DATA) };
  let error = (observed < 0).then(std::io::Error::last_os_error);
  ensure!(
    unsafe { libc::lseek(fd, position, libc::SEEK_SET) } == position,
    "Restore snapshot disk cursor: {}",
    std::io::Error::last_os_error()
  );
  if observed < 0 {
    let error = error.expect("failed extent query has an error");
    return match error.raw_os_error() {
      Some(libc::ENXIO) => Ok(size),
      Some(libc::EINVAL | libc::ENOTSUP | libc::ENOSYS) => Ok(offset as u64),
      _ => Err(error.into()),
    };
  }
  let observed = observed as u64;
  ensure!(
    observed >= offset as u64 && observed < size,
    "Snapshot disk data extent is invalid"
  );
  Ok(observed / CHUNK * CHUNK)
}
