use anyhow::{ensure, Context};
use objc2::AllocAnyThread;
use objc2_foundation::{NSArray, NSDictionary, NSString};
use objc2_virtualization::{
  VZDirectorySharingDeviceConfiguration, VZMultipleDirectoryShare, VZSharedDirectory,
  VZVirtioFileSystemDeviceConfiguration, VZVirtualMachineConfiguration,
};
use std::{
  collections::BTreeSet,
  fs::File,
  os::unix::fs::{MetadataExt, OpenOptionsExt},
  path::{Path, PathBuf},
  sync::Arc,
};

#[derive(Clone)]
pub struct Directory {
  name: String,
  file: Arc<File>,
  path: PathBuf,
  read_only: bool,
}

impl Directory {
  pub fn path(&self) -> &Path {
    &self.path
  }

  pub fn identity(&self) -> anyhow::Result<(u64, u64)> {
    let info = self.file.metadata()?;
    Ok((info.dev(), info.ino()))
  }

  pub fn open(name: &str, path: &Path, read_only: bool) -> anyhow::Result<Self> {
    ensure!(
      !name.is_empty()
        && name.len() <= 64
        && name != "."
        && name != ".."
        && name
          .bytes()
          .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte)),
      "Shared folder name must contain 1–64 letters, digits, dots, underscores or hyphens"
    );
    ensure!(path.is_absolute(), "Shared folder path must be absolute");
    let file = File::options()
      .read(true)
      .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
      .open(path)
      .context("Open authorized shared folder")?;
    ensure!(
      file.metadata()?.is_dir(),
      "Shared folder must be a directory"
    );
    let path = path.canonicalize()?;
    let directory = Self {
      name: name.into(),
      file: Arc::new(file),
      path,
      read_only,
    };
    directory.validate()?;
    Ok(directory)
  }

  pub(super) fn validate(&self) -> anyhow::Result<()> {
    let expected = self.file.metadata()?;
    let actual = std::fs::symlink_metadata(&self.path)?;
    ensure!(
      actual.is_dir()
        && actual.dev() == expected.dev()
        && actual.ino() == expected.ino()
        && self.path.canonicalize()? == self.path,
      "Shared folder changed; select the authorized folder again"
    );
    Ok(())
  }
}

pub fn device_count(vm: &super::Vm) -> usize {
  unsafe { vm.machine.directorySharingDevices().len() }
}

pub(super) fn attach(
  config: &VZVirtualMachineConfiguration,
  directories: &[Directory],
  mac: bool,
) -> anyhow::Result<()> {
  ensure!(
    directories.len() <= 16,
    "A VM supports at most 16 shared folders"
  );
  if directories.is_empty() {
    return Ok(());
  }
  let mut unique = BTreeSet::new();
  let mut names = Vec::new();
  let mut shares = Vec::new();
  for directory in directories {
    ensure!(
      unique.insert(&directory.name),
      "Shared folder names must be unique"
    );
    directory.validate()?;
    let name = NSString::from_str(&directory.name);
    unsafe {
      VZMultipleDirectoryShare::validateName_error(&name)
        .map_err(|error| anyhow::anyhow!("Invalid shared folder name: {error}"))?;
      shares.push(VZSharedDirectory::initWithURL_readOnly(
        VZSharedDirectory::alloc(),
        &*super::config::url(&directory.path)?,
        directory.read_only,
      ));
    }
    names.push(name);
  }
  unsafe {
    let names: Vec<_> = names.iter().map(|name| &**name).collect();
    let shares: Vec<_> = shares.iter().map(|share| &**share).collect();
    let share = VZMultipleDirectoryShare::initWithDirectories(
      VZMultipleDirectoryShare::alloc(),
      &NSDictionary::from_slices(&names, &shares),
    );
    let tag = if mac {
      VZVirtioFileSystemDeviceConfiguration::macOSGuestAutomountTag()
    } else {
      NSString::from_str("hopper")
    };
    let device = VZVirtioFileSystemDeviceConfiguration::initWithTag(
      VZVirtioFileSystemDeviceConfiguration::alloc(),
      &tag,
    );
    device.setShare(Some(&share));
    config.setDirectorySharingDevices(
      &NSArray::<VZDirectorySharingDeviceConfiguration>::from_slice(&[&device]),
    );
  }
  Ok(())
}
