//! only quiescent rings can be captured; popped requests must finish first.

use super::Queue;
use crate::dma::Memory;
use anyhow::ensure;
pub mod wire;

pub struct State {
  size: u16,
  descriptor: u64,
  available: u64,
  used: u64,
  next_available: u16,
  next_used: u16,
  initialized: bool,
}

pub fn capture(queue: &Queue) -> anyhow::Result<State> {
  ensure!(
    queue.pending.is_empty(),
    "Cannot capture a Virtio queue with unfinished requests"
  );
  Ok(State {
    size: queue.size,
    descriptor: queue.descriptor,
    available: queue.available,
    used: queue.used,
    next_available: queue.next_available,
    next_used: queue.next_used,
    initialized: queue.initialized,
  })
}

/// Read-only validation against the matching restored guest RAM, before execution.
pub fn restore(state: &State, memory: &impl Memory) -> anyhow::Result<Queue> {
  let mut queue = super::create(
    memory,
    state.size,
    state.descriptor,
    state.available,
    state.used,
  )?;
  if state.initialized {
    ensure!(
      super::word(memory, state.used + 2)? == state.next_used,
      "Captured Virtio used ring does not match guest RAM"
    );
  }
  ensure!(
    super::word(memory, state.available + 2)?.wrapping_sub(state.next_available) <= state.size,
    "Captured Virtio available ring exceeds capacity"
  );
  queue.next_available = state.next_available;
  queue.next_used = state.next_used;
  queue.initialized = state.initialized;
  Ok(queue)
}
