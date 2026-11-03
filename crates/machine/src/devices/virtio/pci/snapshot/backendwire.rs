//! small input/console records; graphics and storage use dedicated codecs.

use super::{backend, State};
use crate::devices::virtio::{console, input};
use anyhow::{ensure, Context};
use zeroize::Zeroizing;

pub fn limit(saved: &State) -> Option<usize> {
  match &saved.backend {
    Some(backend::State::Input(_)) => Some(input::wire::MAX),
    Some(backend::State::Console(_)) => Some(console::wire::MAX),
    _ => None,
  }
}

pub fn encode(saved: &State) -> anyhow::Result<Option<Zeroizing<Vec<u8>>>> {
  match &saved.backend {
    Some(backend::State::Input(input)) => Ok(Some(input::wire::encode(input)?)),
    Some(backend::State::Console(port)) => Ok(Some(console::wire::encode(port)?)),
    _ => Ok(None),
  }
}

enum Binding {
  Input(input::Kind),
  Console(String),
}

/// Destroy original backend contents before decoding, retaining only its kind/name binding.
pub fn reopen(mut saved: State, bytes: &[u8]) -> anyhow::Result<State> {
  let original = saved
    .backend
    .take()
    .context("Backend record reopening is unavailable")?;
  let binding = match &original {
    backend::State::Input(input) => Binding::Input(input::kind(input)),
    backend::State::Console(port) => Binding::Console(console::name(port).to_owned()),
    _ => anyhow::bail!("Backend record reopening is unavailable"),
  };
  drop(original);
  let reopened = match binding {
    Binding::Input(kind) => {
      let reopened = input::wire::decode(bytes)?;
      ensure!(
        input::kind(&reopened) == kind,
        "Input backend record kind is incompatible"
      );
      backend::State::Input(reopened)
    }
    Binding::Console(name) => {
      let reopened = console::wire::decode(bytes)?;
      ensure!(
        console::name(&reopened) == name,
        "Console backend record name is incompatible"
      );
      backend::State::Console(reopened)
    }
  };
  saved.backend = Some(reopened);
  Ok(saved)
}
