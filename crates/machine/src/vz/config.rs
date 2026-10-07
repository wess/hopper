use super::{Boot, Linux};
use anyhow::{ensure, Context};
use objc2::{rc::Retained, AllocAnyThread};
use objc2_foundation::{NSArray, NSData, NSString, NSURL};
use objc2_virtualization::*;
use std::path::Path;

pub(super) fn linux(boot: &Linux) -> anyhow::Result<Retained<VZVirtualMachineConfiguration>> {
  unsafe {
    let config = base(boot.cpus, boot.memory, boot.width, boot.height)?;
    ensure!(
      boot.identity.len() <= 4096,
      "VZ machine identity exceeds bounds"
    );
    let identity = VZGenericMachineIdentifier::initWithDataRepresentation(
      VZGenericMachineIdentifier::alloc(),
      &NSData::with_bytes(&boot.identity),
    )
    .context("Invalid VZ machine identity")?;
    let platform = VZGenericPlatformConfiguration::new();
    platform.setMachineIdentifier(&identity);
    let loader = loader(&boot.boot)?;
    config.setPlatform(&platform);
    config.setBootLoader(Some(&loader));
    if let Some(console) = &boot.console {
      super::console::attach(&config, console)?;
    }
    let disk = storage(&boot.disk, false)?;
    let media = boot
      .installer
      .as_deref()
      .map(|path| storage(path, true))
      .transpose()?;
    let mut disks: Vec<&VZStorageDeviceConfiguration> = vec![&disk];
    if let Some(media) = &media {
      disks.push(media);
    }
    config.setStorageDevices(&NSArray::from_slice(&disks));
    let graphics = VZVirtioGraphicsDeviceConfiguration::new();
    let scanout = VZVirtioGraphicsScanoutConfiguration::initWithWidthInPixels_heightInPixels(
      VZVirtioGraphicsScanoutConfiguration::alloc(),
      boot.width as isize,
      boot.height as isize,
    );
    graphics.setScanouts(&NSArray::<VZVirtioGraphicsScanoutConfiguration>::from_slice(&[&scanout]));
    config.setGraphicsDevices(&NSArray::<VZGraphicsDeviceConfiguration>::from_slice(&[
      &graphics,
    ]));
    config.setKeyboards(&NSArray::<VZKeyboardConfiguration>::from_slice(&[
      &VZUSBKeyboardConfiguration::new(),
    ]));
    config.setPointingDevices(&NSArray::<VZPointingDeviceConfiguration>::from_slice(&[
      &VZUSBScreenCoordinatePointingDeviceConfiguration::new(),
    ]));
    config.setEntropyDevices(&NSArray::<VZEntropyDeviceConfiguration>::from_slice(&[
      &VZVirtioEntropyDeviceConfiguration::new(),
    ]));
    config
      .validateWithError()
      .map_err(|error| anyhow::anyhow!("Invalid VZ configuration: {error}"))?;
    Ok(config)
  }
}

pub(super) fn base(
  cpus: usize,
  memory: u64,
  width: usize,
  height: usize,
) -> anyhow::Result<Retained<VZVirtualMachineConfiguration>> {
  unsafe {
    ensure!(
      VZVirtualMachine::isSupported(),
      "Virtualization is unavailable on this host"
    );
    ensure!(
      (VZVirtualMachineConfiguration::minimumAllowedCPUCount()
        ..=VZVirtualMachineConfiguration::maximumAllowedCPUCount())
        .contains(&cpus),
      "VZ CPU count exceeds host limits"
    );
    ensure!(
      (VZVirtualMachineConfiguration::minimumAllowedMemorySize()
        ..=VZVirtualMachineConfiguration::maximumAllowedMemorySize())
        .contains(&memory),
      "VZ memory exceeds host limits"
    );
    ensure!(
      (640..=4096).contains(&width) && (480..=4096).contains(&height),
      "VZ display dimensions exceed bounds"
    );
    let config = VZVirtualMachineConfiguration::new();
    config.setCPUCount(cpus);
    config.setMemorySize(memory);
    Ok(config)
  }
}

pub(super) fn storage(
  path: &Path,
  read_only: bool,
) -> anyhow::Result<Retained<VZVirtioBlockDeviceConfiguration>> {
  use std::{io::Read, os::unix::fs::OpenOptionsExt};
  let mut disk = std::fs::OpenOptions::new()
    .read(true)
    .custom_flags(libc::O_NOFOLLOW)
    .open(path)?;
  ensure!(
    disk.metadata()?.is_file(),
    "VZ storage requires a regular file"
  );
  let mut magic = [0; 4];
  disk.read_exact(&mut magic)?;
  ensure!(
    magic != *b"QFI\xfb",
    "Preserve the previous disk for migration; VZ requires a raw guest image"
  );
  unsafe {
    let attachment = VZDiskImageStorageDeviceAttachment::initWithURL_readOnly_error(
      VZDiskImageStorageDeviceAttachment::alloc(),
      &*file(path)?,
      read_only,
    )
    .map_err(|error| anyhow::anyhow!("Attach VZ disk: {error}"))?;
    Ok(VZVirtioBlockDeviceConfiguration::initWithAttachment(
      VZVirtioBlockDeviceConfiguration::alloc(),
      &attachment,
    ))
  }
}

fn loader(boot: &Boot) -> anyhow::Result<Retained<VZBootLoader>> {
  unsafe {
    match boot {
      Boot::Efi { variables } => {
        let loader = VZEFIBootLoader::new();
        let variables =
          VZEFIVariableStore::initWithURL(VZEFIVariableStore::alloc(), &*file(variables)?);
        loader.setVariableStore(Some(&variables));
        Ok(loader.into_super())
      }
      Boot::Kernel {
        kernel,
        initramfs,
        command_line,
      } => {
        super::kernel::file(kernel)?;
        ensure!(
          command_line.len() <= 4096 && !command_line.contains('\0'),
          "VZ kernel command line exceeds bounds"
        );
        let loader =
          VZLinuxBootLoader::initWithKernelURL(VZLinuxBootLoader::alloc(), &*file(kernel)?);
        let initramfs = initramfs.as_deref().map(file).transpose()?;
        loader.setInitialRamdiskURL(initramfs.as_deref());
        loader.setCommandLine(&NSString::from_str(command_line));
        Ok(loader.into_super())
      }
    }
  }
}

pub(super) fn url(path: &Path) -> anyhow::Result<Retained<NSURL>> {
  ensure!(path.is_absolute(), "VZ paths must be absolute");
  let path = path.to_str().context("VZ path is not UTF-8")?;
  ensure!(!path.contains('\0'), "VZ path contains a null byte");
  Ok(NSURL::fileURLWithPath(&NSString::from_str(path)))
}

pub(super) fn file(path: &Path) -> anyhow::Result<Retained<NSURL>> {
  ensure!(
    std::fs::symlink_metadata(path)?.is_file(),
    "VZ requires a regular file, not a symlink"
  );
  url(path)
}

pub fn identity() -> Vec<u8> {
  unsafe {
    VZGenericMachineIdentifier::new()
      .dataRepresentation()
      .to_vec()
  }
}

pub fn create_variables(path: &Path) -> anyhow::Result<()> {
  unsafe {
    VZEFIVariableStore::initCreatingVariableStoreAtURL_options_error(
      VZEFIVariableStore::alloc(),
      &*url(path)?,
      VZEFIVariableStoreInitializationOptions::empty(),
    )
    .map_err(|error| anyhow::anyhow!("Create VZ EFI variables: {error}"))?;
  }
  use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
  let file = std::fs::OpenOptions::new()
    .read(true)
    .custom_flags(libc::O_NOFOLLOW)
    .open(path)?;
  file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
  Ok(())
}
