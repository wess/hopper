use super::{config, restore};
use anyhow::{ensure, Context};
use objc2::{rc::Retained, AllocAnyThread};
use objc2_foundation::{NSArray, NSData};
use objc2_virtualization::*;
use std::path::{Path, PathBuf};

pub struct Mac {
  pub cpus: usize,
  pub memory: u64,
  pub width: usize,
  pub height: usize,
  pub image: restore::Image,
  pub identity: Vec<u8>,
  pub auxiliary: PathBuf,
  pub disk: PathBuf,
}

pub(super) fn config(boot: &Mac) -> anyhow::Result<Retained<VZVirtualMachineConfiguration>> {
  unsafe {
    ensure!(
      boot.cpus >= boot.image.minimum_cpus && boot.memory >= boot.image.minimum_memory,
      "macOS resources are below this restore image's requirements"
    );
    let config = config::base(boot.cpus, boot.memory, boot.width, boot.height)?;
    ensure!(
      boot.identity.len() <= 4096,
      "macOS machine identity exceeds bounds"
    );
    let model = model(&boot.image.hardware)?;
    let identity = VZMacMachineIdentifier::initWithDataRepresentation(
      VZMacMachineIdentifier::alloc(),
      &NSData::with_bytes(&boot.identity),
    )
    .context("Invalid macOS machine identity")?;
    let auxiliary = super::auxiliary::open(&boot.auxiliary, &boot.image.hardware)?;
    let platform = VZMacPlatformConfiguration::new();
    platform.setHardwareModel(&model);
    platform.setMachineIdentifier(&identity);
    platform.setAuxiliaryStorage(Some(&auxiliary));
    config.setPlatform(&platform);
    config.setBootLoader(Some(&VZMacOSBootLoader::new()));
    let disk = config::storage(&boot.disk, false)?;
    config.setStorageDevices(&NSArray::<VZStorageDeviceConfiguration>::from_slice(&[
      &disk,
    ]));
    let graphics = VZMacGraphicsDeviceConfiguration::new();
    let display =
      VZMacGraphicsDisplayConfiguration::initWithWidthInPixels_heightInPixels_pixelsPerInch(
        VZMacGraphicsDisplayConfiguration::alloc(),
        boot.width as isize,
        boot.height as isize,
        80,
      );
    graphics.setDisplays(&NSArray::<VZMacGraphicsDisplayConfiguration>::from_slice(
      &[&display],
    ));
    config.setGraphicsDevices(&NSArray::<VZGraphicsDeviceConfiguration>::from_slice(&[
      &graphics,
    ]));
    config.setKeyboards(&NSArray::<VZKeyboardConfiguration>::from_slice(&[
      &VZUSBKeyboardConfiguration::new(),
    ]));
    config.setPointingDevices(&NSArray::<VZPointingDeviceConfiguration>::from_slice(&[
      &VZUSBScreenCoordinatePointingDeviceConfiguration::new(),
    ]));
    config
      .validateWithError()
      .map_err(|error| anyhow::anyhow!("Invalid macOS VZ configuration: {error}"))?;
    Ok(config)
  }
}

pub(super) fn model(bytes: &[u8]) -> anyhow::Result<Retained<VZMacHardwareModel>> {
  ensure!(bytes.len() <= 65536, "macOS hardware model exceeds bounds");
  unsafe {
    let model = VZMacHardwareModel::initWithDataRepresentation(
      VZMacHardwareModel::alloc(),
      &NSData::with_bytes(bytes),
    )
    .context("Invalid macOS hardware model")?;
    ensure!(
      model.isSupported(),
      "macOS hardware model is unsupported on this host"
    );
    Ok(model)
  }
}

pub fn identity() -> Vec<u8> {
  unsafe { VZMacMachineIdentifier::new().dataRepresentation().to_vec() }
}

pub fn create_auxiliary(path: &Path, hardware: &[u8]) -> anyhow::Result<()> {
  super::auxiliary::create(path, hardware)
}
