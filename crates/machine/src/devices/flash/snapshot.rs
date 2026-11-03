//! command state accompanies the separately captured NVRAM bytes.

use super::{Flash, Mode, Pending};
use anyhow::ensure;
pub mod wire;

pub struct State {
  size: usize,
  block: usize,
  locked: Vec<bool>,
  mode: Mode,
  status: u8,
  pending: Pending,
}

pub fn capture(flash: &Flash) -> State {
  State {
    size: flash.data.len(),
    block: flash.block,
    locked: flash.locked.clone(),
    mode: flash.mode,
    status: flash.status,
    pending: flash.pending.clone(),
  }
}

/// Reconstruct before guest execution using the matching captured NVRAM bytes.
/// A fresh bank keeps reconstruction separate from any recoverable running state.
pub fn restore(state: &State, data: Vec<u8>) -> anyhow::Result<Flash> {
  wire::validate(state)?;
  ensure!(
    data.len() == state.size,
    "Captured flash bank size does not match its command state"
  );
  let mut flash = super::create(data, state.block)?;
  flash.locked.clone_from(&state.locked);
  flash.mode = state.mode;
  flash.status = state.status;
  flash.pending = state.pending.clone();
  Ok(flash)
}
