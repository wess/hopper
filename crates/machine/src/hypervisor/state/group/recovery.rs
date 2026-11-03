use super::{clock, Captured};
use crate::hypervisor::{pmu, Cpu};

pub(crate) fn resume(
  cpu: &mut Cpu<'_>,
  saved: &Captured,
  access: pmu::Token<'_>,
  epoch: u64,
) -> anyhow::Result<()> {
  // exclude the initial held interval before temporary execution can update timers.
  clock::restore(cpu, &saved.clock, epoch)?;
  access.restore(cpu, &saved.pmu)?;
  clock::restore(cpu, &saved.clock, epoch)
}
