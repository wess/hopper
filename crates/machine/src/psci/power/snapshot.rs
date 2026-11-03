use super::{Power, State};
use anyhow::ensure;
use std::ops::Range;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
  pub(crate) cores: Vec<(u64, bool)>,
  pub(crate) executable: Vec<Range<u64>>,
}

impl Snapshot {
  pub fn count(&self) -> usize {
    self.cores.len()
  }

  pub fn online(&self, index: usize) -> anyhow::Result<bool> {
    self
      .cores
      .get(index)
      .map(|core| core.1)
      .ok_or_else(|| anyhow::anyhow!("Unknown captured CPU"))
  }
}

/// Only at a coordinated owner checkpoint, after startup and trap replies drain.
pub fn capture(power: &Power) -> anyhow::Result<Snapshot> {
  ensure!(
    power.cores.iter().all(|core| core.1 != State::Pending),
    "CPU startup is still pending"
  );
  ensure!(
    power.cores.first().is_some_and(|core| core.1 == State::On),
    "Primary CPU must remain on"
  );
  Ok(Snapshot {
    cores: power
      .cores
      .iter()
      .map(|&(affinity, state)| (affinity, state == State::On))
      .collect(),
    executable: power.executable.clone(),
  })
}

pub fn validate(power: &Power, snapshot: &Snapshot) -> anyhow::Result<()> {
  ensure!(
    power.cores.len() == snapshot.cores.len()
      && power
        .cores
        .iter()
        .zip(&snapshot.cores)
        .all(|(current, saved)| current.0 == saved.0)
      && power.executable == snapshot.executable,
    "CPU power snapshot topology does not match"
  );
  ensure!(
    power.cores.iter().all(|core| core.1 != State::Pending),
    "CPU startup is still pending"
  );
  ensure!(
    snapshot.cores.first().is_some_and(|core| core.1),
    "Captured primary CPU is not on"
  );
  Ok(())
}

/// Only before reconstructed owners resume. Validate all hardware separately first.
pub fn restore(power: &mut Power, snapshot: &Snapshot) -> anyhow::Result<()> {
  validate(power, snapshot)?;
  for (current, saved) in power.cores.iter_mut().zip(&snapshot.cores) {
    current.1 = if saved.1 { State::On } else { State::Off };
  }
  Ok(())
}
