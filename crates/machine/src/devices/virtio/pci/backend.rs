use super::{block, gpu, queue, BLOCK_SIZE, FLUSH, READONLY, VERSION};
use crate::dma::Memory;

pub(super) enum Backend {
  Disk(block::Disk),
  Gpu(gpu::Display),
}

pub(super) fn config_length(backend: &Backend) -> u64 {
  match backend {
    Backend::Disk(_) => 64,
    Backend::Gpu(_) => 16,
  }
}

pub(super) fn features(backend: &Backend) -> u64 {
  match backend {
    Backend::Disk(disk) => {
      VERSION | FLUSH | BLOCK_SIZE | if block::readonly(disk) { READONLY } else { 0 }
    }
    Backend::Gpu(_) => VERSION,
  }
}

pub(super) fn config(backend: &Backend, bytes: &mut [u8; 64]) {
  match backend {
    Backend::Disk(disk) => {
      bytes[..8].copy_from_slice(&block::capacity(disk).to_le_bytes());
      bytes[20..24].copy_from_slice(&512u32.to_le_bytes());
    }
    Backend::Gpu(_) => bytes[8..12].copy_from_slice(&1u32.to_le_bytes()),
  }
}

pub(super) fn status(backend: &mut Backend, features: u64, reset: bool) -> anyhow::Result<()> {
  match backend {
    Backend::Disk(disk) => block::writeback(disk, !reset && features & FLUSH != 0)?,
    Backend::Gpu(display) if reset => gpu::reset(display),
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
  }
}
