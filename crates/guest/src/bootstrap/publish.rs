use anyhow::ensure;
use model::guest::wire::auth::Material;
use std::{
  fs::{File, OpenOptions},
  os::{
    fd::{AsRawFd, FromRawFd},
    unix::fs::{MetadataExt, OpenOptionsExt},
  },
  sync::atomic::{AtomicBool, Ordering},
};

pub(super) fn directory() -> anyhow::Result<File> {
  let run = OpenOptions::new()
    .read(true)
    .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
    .open("/run")?;
  let info = run.metadata()?;
  let mut filesystem: libc::statfs = unsafe { std::mem::zeroed() };
  ensure!(
    info.uid() == 0
      && info.gid() == 0
      && info.mode() & 0o022 == 0
      && unsafe { libc::fstatfs(run.as_raw_fd(), &mut filesystem) } == 0
      && filesystem.f_type as u64 == 0x01021994,
    "Boot credentials require private runtime storage"
  );
  if unsafe { libc::mkdirat(run.as_raw_fd(), c"hopper".as_ptr(), 0o700) } != 0 {
    ensure!(
      std::io::Error::last_os_error().kind() == std::io::ErrorKind::AlreadyExists,
      "Create boot credential directory"
    );
  }
  let descriptor = unsafe {
    libc::openat(
      run.as_raw_fd(),
      c"hopper".as_ptr(),
      libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
    )
  };
  ensure!(descriptor >= 0, "Open boot credential directory");
  let directory = unsafe { File::from_raw_fd(descriptor) };
  let info = directory.metadata()?;
  ensure!(
    info.uid() == 0 && info.gid() == 0 && info.mode() & 0o7777 == 0o700,
    "Boot credential directory is not private"
  );
  ensure!(
    unsafe { libc::flock(directory.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
    "Boot credential publication is already owned"
  );
  Ok(directory)
}

pub(super) fn exists(directory: &File) -> anyhow::Result<bool> {
  let entries = std::fs::read_dir(format!("/proc/self/fd/{}", directory.as_raw_fd()))?
    .take(2)
    .collect::<std::io::Result<Vec<_>>>()?;
  if entries.is_empty() {
    return Ok(false);
  }
  ensure!(
    entries.len() == 1 && entries[0].file_name() == "boot",
    "Preserve unknown or interrupted boot credential state"
  );
  crate::daemon::credentials::read(directory)?;
  Ok(true)
}

pub(super) fn write(
  directory: &File,
  material: &Material,
  stop: &AtomicBool,
) -> anyhow::Result<()> {
  ensure!(!stop.load(Ordering::Acquire), "Boot delivery cancelled");
  let descriptor = unsafe {
    libc::openat(
      directory.as_raw_fd(),
      c"pending".as_ptr(),
      libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
      0o400,
    )
  };
  ensure!(descriptor >= 0, "Create private boot credential stage");
  let mut file = unsafe { File::from_raw_fd(descriptor) };
  material.write(&mut file)?;
  file.sync_all()?;
  ensure!(!stop.load(Ordering::Acquire), "Boot delivery cancelled");
  ensure!(
    unsafe {
      libc::syscall(
        libc::SYS_renameat2,
        directory.as_raw_fd(),
        c"pending".as_ptr(),
        directory.as_raw_fd(),
        c"boot".as_ptr(),
        libc::RENAME_NOREPLACE,
      )
    } == 0,
    "Publish boot credentials without replacement"
  );
  directory.sync_all()?;
  Ok(())
}
