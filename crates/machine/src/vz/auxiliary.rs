use super::{config, mac};
use anyhow::{ensure, Context};
use objc2::{rc::Retained, AllocAnyThread};
use objc2_virtualization::{VZMacAuxiliaryStorage, VZMacAuxiliaryStorageInitializationOptions};
use std::{
  ffi::CString,
  fs::{File, OpenOptions},
  io::{Read, Write},
  os::unix::{
    ffi::OsStrExt,
    fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
  },
  path::Path,
};

pub(super) fn create(root: &Path, hardware: &[u8]) -> anyhow::Result<()> {
  config::url(root)?;
  let model = mac::model(hardware)?;
  let parent = root.parent().context("Auxiliary storage needs a parent")?;
  let stage = tempfile::Builder::new()
    .prefix("auxiliary")
    .tempdir_in(parent)?;
  std::fs::set_permissions(stage.path(), std::fs::Permissions::from_mode(0o700))?;
  let state = stage.path().join("state");
  unsafe {
    VZMacAuxiliaryStorage::initCreatingStorageAtURL_hardwareModel_options_error(
      VZMacAuxiliaryStorage::alloc(),
      &*config::url(&state)?,
      &model,
      VZMacAuxiliaryStorageInitializationOptions::empty(),
    )
    .map_err(|error| anyhow::anyhow!("Create macOS auxiliary storage: {error}"))?;
  }
  let state = OpenOptions::new()
    .read(true)
    .custom_flags(libc::O_NOFOLLOW)
    .open(&state)?;
  ensure!(
    state.metadata()?.is_file(),
    "Created auxiliary storage must be a regular file"
  );
  state.set_permissions(std::fs::Permissions::from_mode(0o600))?;
  state.sync_all()?;
  let mut binding = OpenOptions::new()
    .create_new(true)
    .write(true)
    .mode(0o600)
    .open(stage.path().join("hardware"))?;
  binding.write_all(hardware)?;
  binding.sync_all()?;
  File::open(stage.path())?.sync_all()?;
  let from = CString::new(stage.path().as_os_str().as_bytes())?;
  let to = CString::new(root.as_os_str().as_bytes())?;
  if unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) } != 0 {
    return Err(std::io::Error::last_os_error())
      .context("Publish macOS auxiliary storage without replacing data");
  }
  let _ = stage.keep();
  File::open(parent)?.sync_all()?;
  Ok(())
}

pub(super) fn open(
  root: &Path,
  hardware: &[u8],
) -> anyhow::Result<Retained<VZMacAuxiliaryStorage>> {
  let metadata = std::fs::symlink_metadata(root)?;
  ensure!(
    metadata.is_dir()
      && metadata.uid() == unsafe { libc::geteuid() }
      && metadata.mode() & 0o077 == 0,
    "macOS auxiliary storage must be a private owned directory"
  );
  let binding = read(&root.join("hardware"))?;
  ensure!(
    binding.metadata()?.len() <= 65536,
    "Auxiliary hardware binding exceeds bounds"
  );
  let mut bytes = Vec::new();
  binding.take(65537).read_to_end(&mut bytes)?;
  ensure!(
    bytes == hardware,
    "Auxiliary storage belongs to a different macOS hardware model"
  );
  read(&root.join("state"))?;
  unsafe {
    Ok(VZMacAuxiliaryStorage::initWithURL(
      VZMacAuxiliaryStorage::alloc(),
      &*config::file(&root.join("state"))?,
    ))
  }
}

fn read(path: &Path) -> anyhow::Result<File> {
  let file = OpenOptions::new()
    .read(true)
    .custom_flags(libc::O_NOFOLLOW)
    .open(path)?;
  let metadata = file.metadata()?;
  ensure!(
    metadata.is_file()
      && metadata.uid() == unsafe { libc::geteuid() }
      && metadata.mode() & 0o077 == 0,
    "Auxiliary files must be private owned regular files"
  );
  Ok(file)
}
