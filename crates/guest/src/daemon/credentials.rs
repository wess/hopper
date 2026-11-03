use anyhow::ensure;
use model::guest::wire::auth::Material;
use std::{
  fs::{File, OpenOptions},
  os::{
    fd::{AsRawFd, FromRawFd},
    unix::fs::{MetadataExt, OpenOptionsExt},
  },
};

pub(super) fn load() -> anyhow::Result<Material> {
  let directory = OpenOptions::new()
    .read(true)
    .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
    .open("/run/hopper")?;
  let info = directory.metadata()?;
  ensure!(
    info.is_dir() && info.uid() == 0 && info.gid() == 0 && info.mode() & 0o7777 == 0o700,
    "Guest credential directory is not private"
  );
  read(&directory)
}

pub(crate) fn read(directory: &File) -> anyhow::Result<Material> {
  let fd = unsafe {
    libc::openat(
      directory.as_raw_fd(),
      c"boot".as_ptr(),
      libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
    )
  };
  ensure!(
    fd >= 0,
    "Open guest boot credentials: {}",
    std::io::Error::last_os_error()
  );
  let mut file = unsafe { File::from_raw_fd(fd) };
  let info = file.metadata()?;
  ensure!(
    info.is_file()
      && info.uid() == 0
      && info.gid() == 0
      && info.nlink() == 1
      && info.mode() & 0o7777 == 0o400
      && info.len() == Material::LENGTH as u64,
    "Guest boot credentials are not protected or bounded"
  );
  let material = Material::read(&mut file)?;
  ensure!(
    material.boot.guest == model::GuestOs::Linux,
    "Guest boot credentials require Linux"
  );
  Ok(material)
}
