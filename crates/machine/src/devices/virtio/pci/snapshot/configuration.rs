//! immutable hardware properties, excluding guest-selected transport and backend state.

use super::{Backend, Device};
use crate::devices::virtio::{block, console, gpu, input, scsi};
use sha2::{Digest, Sha256};

pub fn fingerprint(device: &Device) -> [u8; 32] {
  let mut hash = Sha256::new();
  hash.update(b"hopper.virtio.configuration.v1");
  hash.update([super::kind(&device.backend)]);
  hash.update(device.pci.configuration());
  hash.update(super::super::features(device).to_le_bytes());
  hash.update((device.window_offset as u64).to_le_bytes());
  hash.update((device.queues.len() as u64).to_le_bytes());
  hash.update((super::msix::count(&device.msix) as u64).to_le_bytes());
  match &device.backend {
    Backend::Disk(disk) => {
      hash.update(block::capacity(disk).to_le_bytes());
      hash.update([block::readonly(disk) as u8]);
      hash.update(block::identity(disk));
    }
    Backend::Gpu(display) => {
      let (width, height) = gpu::preferred(display);
      hash.update(width.to_le_bytes());
      hash.update(height.to_le_bytes());
    }
    Backend::Input(controller) => hash.update([match input::kind(controller) {
      input::Kind::Keyboard => 0,
      input::Kind::Tablet => 1,
    }]),
    Backend::Optical(media) => hash.update(scsi::capacity(media).to_le_bytes()),
    Backend::Console(port) => {
      let name = console::name(port).as_bytes();
      hash.update((name.len() as u64).to_le_bytes());
      hash.update(name);
    }
  }
  hash.finalize().into()
}
