use anyhow::{ensure, Context};
use objc2::AllocAnyThread;
use objc2_foundation::{NSArray, NSString};
use objc2_virtualization::{
  VZMACAddress, VZNATNetworkDeviceAttachment, VZNetworkDeviceConfiguration,
  VZVirtioNetworkDeviceConfiguration, VZVirtualMachineConfiguration,
};
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
  Nat,
  Disconnected,
}

pub fn address(identity: &[u8]) -> anyhow::Result<String> {
  ensure!(
    !identity.is_empty() && identity.len() <= 4096,
    "VZ network identity exceeds bounds"
  );
  let mut hash = Sha256::new();
  hash.update(b"hopper.vz.network.v1\0");
  hash.update(identity);
  let mut bytes = hash.finalize();
  bytes[0] = (bytes[0] & 0xfc) | 0x02;
  Ok(
    bytes[..6]
      .iter()
      .map(|byte| format!("{byte:02x}"))
      .collect::<Vec<_>>()
      .join(":"),
  )
}

pub(super) fn attach(
  config: &VZVirtualMachineConfiguration,
  identity: &[u8],
  mode: Mode,
) -> anyhow::Result<()> {
  let address = address(identity)?;
  unsafe {
    let mac = VZMACAddress::initWithString(VZMACAddress::alloc(), &NSString::from_str(&address))
      .context("Invalid VZ network address")?;
    ensure!(
      mac.isUnicastAddress() && mac.isLocallyAdministeredAddress(),
      "VZ requires a local unicast network address"
    );
    let device = VZVirtioNetworkDeviceConfiguration::new();
    device.setMACAddress(&mac);
    if mode == Mode::Nat {
      device.setAttachment(Some(&VZNATNetworkDeviceAttachment::new()));
    }
    config.setNetworkDevices(&NSArray::<VZNetworkDeviceConfiguration>::from_slice(&[
      &device,
    ]));
  }
  Ok(())
}
