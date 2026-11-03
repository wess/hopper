use super::read;
use crate::hypervisor::Cpu;
use anyhow::ensure;

pub(super) const REGISTERS: [u16; 12] = [
  0xc000, 0xc020, 0xc021, 0xc024, 0xc025, 0xc028, 0xc029, 0xc030, 0xc031, 0xc038, 0xc039, 0xc03a,
];

pub(super) fn capture(cpu: &Cpu<'_>) -> anyhow::Result<[u64; 12]> {
  let mut values = [0; 12];
  for (register, value) in REGISTERS.into_iter().zip(&mut values) {
    *value = read(cpu, register)?;
  }
  validate(&values)?;
  Ok(values)
}

pub(super) fn validate(values: &[u64; 12]) -> anyhow::Result<()> {
  // wider vector state is not represented by the 128-bit SIMD bank.
  ensure!(
    (values[1] >> 32) & 15 == 0,
    "CPU snapshot does not yet cover SVE state"
  );
  ensure!(
    (values[2] >> 24) & 15 == 0,
    "CPU snapshot does not yet cover SME state"
  );
  Ok(())
}
