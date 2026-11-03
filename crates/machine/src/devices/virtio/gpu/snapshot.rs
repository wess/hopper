//! bounded graphics metadata; immutable pixel/frame buffers are shared until writes.

use super::{linear, Display};
use crate::dma::Memory;
use anyhow::ensure;
pub mod wire;

pub(crate) fn preferred(saved: &State) -> (u32, u32) {
  saved.0.preferred
}

pub struct State(Display);

pub fn capture(display: &Display) -> State {
  State(display.clone())
}

pub fn restore(state: &State, target: &Display, memory: &impl Memory) -> anyhow::Result<Display> {
  ensure!(
    state.0.preferred == target.preferred,
    "Captured display configuration does not match"
  );
  for resource in state.0.resources.values() {
    ensure!(
      resource
        .backing
        .iter()
        .all(|span| memory.contains(span.address, span.length as usize)),
      "Captured graphics backing exceeds restored RAM"
    );
  }
  linear::validate(&state.0.linear, memory)?;
  Ok(state.0.clone())
}
