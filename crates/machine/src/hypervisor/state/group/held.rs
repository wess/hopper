use super::{wire, Captured, Group};
use anyhow::ensure;
use std::time::Duration;

impl Group {
  /// CPU owners remain behind their original recovery barrier; no guest state has executed.
  pub(crate) fn restored(&self, pause: u64, saved: &[Captured]) -> anyhow::Result<()> {
    let mut inner = self.lock()?;
    ensure!(
      saved.len() == self.count,
      "Held checkpoint CPU count differs"
    );
    if inner.pause != Some(pause) {
      return Ok(());
    }
    let Some(pending) = &mut inner.pending else {
      return Ok(());
    };
    if pending.pause != pause
      || pending.started.elapsed() >= Duration::from_secs(5)
      || !matches!(pending.primary, Some(Ok(_)))
    {
      return Ok(());
    }
    for (index, saved) in saved.iter().enumerate() {
      ensure!(
        saved.pmu.binding().0 == index as u64,
        "Held CPU affinity differs from its slot"
      );
      if pending.slots[index].is_none() {
        pending.slots[index] = Some(Ok(wire::decode(&wire::encode(saved)?)?));
      }
    }
    Ok(())
  }
}
