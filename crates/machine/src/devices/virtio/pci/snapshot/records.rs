//! reconstruct saved devices against fresh authorized hardware, without original captures.

use super::{backend, wire, Backend, Device, State};
use crate::devices::virtio::{console, gpu, input};
use anyhow::ensure;
use std::io::Read;

pub enum Contents<'a> {
  Bytes(&'a [u8]),
  Graphics(&'a mut dyn Read),
  #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
  Image {
    directory: &'a std::fs::File,
    bytes: &'a [u8],
  },
}

pub enum Format {
  Bytes(usize),
  Graphics,
  Image,
}

pub fn kind(expected: &Device) -> u8 {
  super::kind(&expected.backend)
}

pub fn format(expected: &Device) -> Format {
  match &expected.backend {
    Backend::Input(_) => Format::Bytes(input::wire::MAX),
    Backend::Console(_) => Format::Bytes(console::wire::MAX),
    Backend::Gpu(_) => Format::Graphics,
    Backend::Disk(_) | Backend::Optical(_) => Format::Image,
  }
}

/// The caller supplies hardware built from the current authorized VM configuration.
/// Disk/media target contents are not captured or trusted as snapshot contents.
pub fn decode(
  transport: &[u8],
  contents: Contents<'_>,
  expected: &Device,
  mut authorize: impl FnMut() -> anyhow::Result<()>,
) -> anyhow::Result<State> {
  authorize()?;
  let mut saved = wire::decode(transport, expected)?;
  authorize()?;
  let reopened = match (&expected.backend, contents) {
    (Backend::Input(target), Contents::Bytes(bytes)) => {
      let reopened = input::wire::decode(bytes)?;
      ensure!(
        input::same_kind(&reopened, target),
        "Input backend record kind is incompatible"
      );
      backend::State::Input(reopened)
    }
    (Backend::Console(target), Contents::Bytes(bytes)) => {
      let reopened = console::wire::decode(bytes)?;
      ensure!(
        console::same_name(&reopened, target),
        "Console backend record name is incompatible"
      );
      backend::State::Console(reopened)
    }
    (Backend::Gpu(target), Contents::Graphics(mut stream)) => backend::State::Gpu(
      gpu::snapshot::wire::decode(&mut stream, gpu::preferred(target), &mut authorize)?,
    ),
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    (Backend::Disk(target), Contents::Image { directory, bytes }) => {
      use crate::devices::virtio::block::snapshot::record;
      backend::State::Disk(record::load(
        directory,
        bytes,
        record::target(target),
        &mut authorize,
      )?)
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    (Backend::Optical(target), Contents::Image { directory, bytes }) => {
      use crate::devices::virtio::scsi::snapshot::record;
      backend::State::Optical(record::load(
        directory,
        bytes,
        record::target(target),
        &mut authorize,
      )?)
    }
    _ => anyhow::bail!("Backend record does not match the authorized hardware"),
  };
  authorize()?;
  saved.backend = Some(reopened);
  Ok(saved)
}

pub fn saved_kind(state: &super::State) -> u8 {
  state.kind
}
