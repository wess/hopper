use crate::machines::{
  native::{
    config, installation,
    restore::{self, Phase},
    State,
  },
  Machines,
};
use model::native::Installation;

pub(super) struct Recovery {
  pub state: Option<&'static str>,
  pub progress: &'static str,
}

pub(super) fn status(
  manager: &Machines,
  id: &str,
  installation: Option<&Installation>,
  hardware: &State,
) -> Option<Recovery> {
  let selected =
    config::paths(&manager.root, id).and_then(|paths| restore::selection(&paths.root, id));
  let (state, progress) = match selected {
    Ok(None) => return None,
    Err(_) => (
      Some("Recovery required"),
      "Restore metadata needs recovery; VM data is retained",
    ),
    Ok(Some(record)) if record.phase != Phase::Active => (
      Some("Recovery required"),
      "Restore was interrupted; its prior snapshot and VM data are retained",
    ),
    Ok(Some(_))
      if !matches!(
        installation::boot_stage(installation),
        Ok(config::Stage::System)
      ) =>
    {
      (
        Some("Recovery required"),
        "Restored VM setup state needs recovery; its snapshots and data are retained",
      )
    }
    Ok(Some(_)) => (
      None,
      match hardware {
        State::Running => "Running from restored state",
        State::Paused => "Restored VM is paused",
        State::Failed(_) => "Restored VM failed; its prior snapshot and data are retained",
        State::Stopped(_) => "Restored state is selected",
      },
    ),
  };
  Some(Recovery {
    // live hardware keeps its lifecycle controls even when its recovery metadata needs attention.
    state: if matches!(hardware, State::Running | State::Paused) {
      None
    } else {
      state
    },
    progress,
  })
}
