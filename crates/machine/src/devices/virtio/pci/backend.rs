use super::{block, gpu, input, queue, scsi, BLOCK_SIZE, FLUSH, READONLY, VERSION};
use crate::dma::Memory;

pub(super) enum Backend {
  Disk(block::Disk),
  Gpu(gpu::Display),
  Input(input::Input),
  Optical(scsi::Optical),
}

pub(super) fn config_length(backend: &Backend) -> u64 {
  match backend {
    Backend::Disk(_) => 64,
    Backend::Gpu(_) => 16,
    Backend::Input(_) => 136,
    Backend::Optical(_) => 36,
  }
}

pub(super) fn features(backend: &Backend) -> u64 {
  match backend {
    Backend::Disk(disk) => {
      VERSION | FLUSH | BLOCK_SIZE | if block::readonly(disk) { READONLY } else { 0 }
    }
    Backend::Gpu(_) | Backend::Input(_) | Backend::Optical(_) => VERSION,
  }
}

pub(super) fn config(backend: &Backend) -> Vec<u8> {
  let mut bytes = vec![0; config_length(backend) as usize];
  match backend {
    Backend::Disk(disk) => {
      bytes[..8].copy_from_slice(&block::capacity(disk).to_le_bytes());
      bytes[20..24].copy_from_slice(&512u32.to_le_bytes());
    }
    Backend::Gpu(_) => bytes[8..12].copy_from_slice(&1u32.to_le_bytes()),
    Backend::Input(input) => bytes.copy_from_slice(&input::config(input)),
    Backend::Optical(media) => bytes.copy_from_slice(&scsi::config(media)),
  }
  bytes
}

pub(super) fn configure(backend: &mut Backend, offset: usize, width: usize, value: u32) {
  if let Backend::Input(input) = backend {
    input::configure(input, offset, width, value);
  }
  if let Backend::Optical(media) = backend {
    scsi::configure(media, offset, width, value);
  }
}

pub(super) fn ready(backend: &Backend, index: usize) -> bool {
  match backend {
    Backend::Input(input) if index == 0 => input::pending(input) > 0,
    Backend::Optical(_) if index == 1 => false,
    _ => true,
  }
}

pub(super) fn status(backend: &mut Backend, features: u64, reset: bool) -> anyhow::Result<()> {
  match backend {
    Backend::Disk(disk) => block::writeback(disk, !reset && features & FLUSH != 0)?,
    Backend::Gpu(display) if reset => gpu::reset(display),
    Backend::Input(input) if reset => input::reset(input),
    Backend::Optical(media) if reset => scsi::reset(media),
    _ => {}
  }
  Ok(())
}

pub(super) fn execute(
  backend: &mut Backend,
  memory: &mut impl Memory,
  chain: &queue::Chain,
  index: usize,
) -> anyhow::Result<u32> {
  match backend {
    Backend::Disk(disk) => block::execute(disk, memory, chain),
    Backend::Gpu(display) if index == 0 => gpu::execute(display, memory, chain),
    Backend::Gpu(display) => gpu::execute_cursor(display, memory, chain),
    Backend::Input(input) => input::execute(input, memory, chain, index),
    Backend::Optical(media) => scsi::execute(media, memory, chain, index),
  }
}
