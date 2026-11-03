//! graphics records stream independently of small transport records.

use super::{backend, State};
use crate::devices::virtio::gpu::snapshot::{self, wire};
use anyhow::Context;
use std::io::{Read, Write};

pub const MAX: u64 = wire::MAX;

pub fn present(saved: &State) -> bool {
  matches!(saved.backend, Some(backend::State::Gpu(_)))
}

pub fn encode(
  saved: &State,
  output: &mut impl Write,
  authorize: impl FnMut() -> anyhow::Result<()>,
) -> anyhow::Result<()> {
  let Some(backend::State::Gpu(gpu)) = &saved.backend else {
    anyhow::bail!("GPU backend record is unavailable");
  };
  wire::encode(gpu, output, authorize)
}

pub fn reopen(
  mut saved: State,
  input: &mut impl Read,
  authorize: impl FnMut() -> anyhow::Result<()>,
) -> anyhow::Result<State> {
  let original = saved
    .backend
    .take()
    .context("GPU backend record is unavailable")?;
  let backend::State::Gpu(gpu) = original else {
    anyhow::bail!("GPU backend record is unavailable");
  };
  let expected = snapshot::preferred(&gpu);
  drop(gpu);
  saved.backend = Some(backend::State::Gpu(wire::decode(
    input, expected, authorize,
  )?));
  Ok(saved)
}
