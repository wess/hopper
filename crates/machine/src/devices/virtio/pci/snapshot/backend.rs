use super::Backend;
use crate::{
  devices::virtio::{console, gpu, input},
  dma::Memory,
};
use anyhow::ensure;

pub(super) enum State {
  Input(input::Input),
  Console(console::Port),
  Gpu(gpu::snapshot::State),
  #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
  Disk(crate::devices::virtio::block::snapshot::State),
  #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
  Optical(crate::devices::virtio::scsi::snapshot::State),
}

pub(super) fn capture(backend: &Backend) -> anyhow::Result<Option<State>> {
  Ok(match backend {
    Backend::Input(input) => Some(State::Input(input.clone())),
    Backend::Console(port) => Some(State::Console(port.clone())),
    Backend::Gpu(display) => Some(State::Gpu(gpu::snapshot::capture(display))),
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    Backend::Disk(disk) => Some(State::Disk(
      crate::devices::virtio::block::snapshot::capture(disk)?,
    )),
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    Backend::Optical(media) => Some(State::Optical(
      crate::devices::virtio::scsi::snapshot::capture(media)?,
    )),
    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    _ => None,
  })
}

pub(super) fn restore(
  state: &State,
  destination: &Backend,
  memory: &impl Memory,
) -> anyhow::Result<Backend> {
  match (state, destination) {
    (State::Input(saved), Backend::Input(target)) => {
      ensure!(
        input::same_kind(saved, target),
        "Captured input kind does not match the device"
      );
      Ok(Backend::Input(saved.clone()))
    }
    (State::Console(saved), Backend::Console(target)) => {
      ensure!(
        console::same_name(saved, target),
        "Captured serial port name does not match the device"
      );
      Ok(Backend::Console(saved.clone()))
    }
    (State::Gpu(saved), Backend::Gpu(target)) => {
      Ok(Backend::Gpu(gpu::snapshot::restore(saved, target, memory)?))
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    (State::Disk(saved), Backend::Disk(target)) => Ok(Backend::Disk(
      crate::devices::virtio::block::snapshot::restore(saved, target)?,
    )),
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    (State::Optical(saved), Backend::Optical(target)) => Ok(Backend::Optical(
      crate::devices::virtio::scsi::snapshot::restore(saved, target)?,
    )),
    _ => anyhow::bail!("Captured backend does not match the device"),
  }
}
