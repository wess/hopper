use anyhow::{ensure, Context};
use std::{
  ffi::CString,
  fs::{File, OpenOptions},
  os::unix::{
    ffi::OsStrExt,
    fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
  },
  path::Path,
};

pub(crate) fn directory(path: &Path) -> anyhow::Result<()> {
  let info = std::fs::symlink_metadata(path)?;
  ensure!(
    info.is_dir() && info.uid() == unsafe { libc::geteuid() } && info.mode() & 0o077 == 0,
    "VZ directory must be private and owned"
  );
  Ok(())
}

pub(super) fn parent(path: &Path) -> anyhow::Result<()> {
  match std::fs::symlink_metadata(path) {
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
      std::fs::DirBuilder::new().mode(0o700).create(path)?;
    }
    Err(error) => return Err(error.into()),
    Ok(_) => {}
  }
  directory(path)
}

pub(crate) fn read(path: &Path, maximum: u64) -> anyhow::Result<File> {
  let file = OpenOptions::new()
    .read(true)
    .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
    .open(path)?;
  let info = file.metadata()?;
  ensure!(
    info.is_file()
      && info.uid() == unsafe { libc::geteuid() }
      && info.mode() & 0o077 == 0
      && (1..=maximum).contains(&info.len()),
    "VZ file must be private, owned and bounded"
  );
  Ok(file)
}

pub(super) fn write(path: &Path) -> anyhow::Result<File> {
  Ok(
    OpenOptions::new()
      .write(true)
      .create_new(true)
      .mode(0o600)
      .open(path)?,
  )
}

pub(super) fn publish(stage: tempfile::TempDir, target: &Path) -> anyhow::Result<()> {
  File::open(stage.path())?.sync_all()?;
  let from = CString::new(stage.path().as_os_str().as_bytes())?;
  let to = CString::new(target.as_os_str().as_bytes())?;
  if unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) } != 0 {
    return Err(std::io::Error::last_os_error())
      .context("Publish VZ state without replacing existing data");
  }
  let _ = stage.keep();
  File::open(target.parent().context("VZ state needs a parent")?)?.sync_all()?;
  Ok(())
}
