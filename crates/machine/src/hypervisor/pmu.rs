//! fixed architectural PMU accesses on a held CPU owner; not a snapshot endpoint.

mod context;
pub mod wire;

#[cfg(test)]
#[path = "../../tests/pmu/cancel.rs"]
mod cancel;
#[cfg(test)]
#[path = "../../tests/pmu/probe.rs"]
mod tests;

use super::{memory, protect, write, Cpu, Memory, Vm};
use anyhow::{ensure, Context};
use std::marker::PhantomData;
use zeroize::{Zeroize, Zeroizing};

const REGISTERS: [u16; 13] = [
  0xdce0, 0xdce1, 0xdce2, 0xdce3, 0xdce5, 0xdce8, 0xdcf0, 0xc4f1, 0xc4f2, 0xdf7f, 0xdcf3, 0xc028,
  0xdf00,
];
const SAVED: [u16; 8] = [
  0xdce0, 0xdce1, 0xdce3, 0xdce5, 0xdce8, 0xdcf0, 0xc4f1, 0xdf7f,
];

pub struct Access<'vm> {
  memory: Memory<'vm>,
}

/// A borrowed code mapping may cross CPU-owner threads; the mapping outlives each token.
/// ```compile_fail
/// let vm = machine::hypervisor::create().unwrap();
/// let token = {
///   let code = machine::hypervisor::pmu::prepare(&vm, 0x3fff0000).unwrap();
///   code.token()
/// };
/// let mut cpu = machine::hypervisor::cpu(&vm).unwrap();
/// token.capture(&mut cpu).unwrap();
/// ```
#[derive(Clone, Copy)]
pub struct Token<'a> {
  address: u64,
  _owner: PhantomData<&'a ()>,
}

pub struct State {
  values: [u64; 8],
  features: u64,
  affinity: u64,
  frequency: u64,
}

impl State {
  pub(crate) fn binding(&self) -> (u64, u64, u64) {
    (self.affinity, self.features, self.frequency)
  }

  fn validate(&self) -> anyhow::Result<()> {
    ensure!(
      (self.features >> 8) & 15 == 1 && (self.values[0] >> 11) & 31 == 0,
      "PMU snapshot configuration is unsupported"
    );
    ensure!(
      (1..=10_000_000_000).contains(&self.frequency),
      "PMU timer frequency is unsupported"
    );
    Ok(())
  }
}

impl Drop for State {
  fn drop(&mut self) {
    self.values.zeroize();
  }
}

/// Caller reserves a nonoverlapping physical page outside guest RAM and devices.
/// The private code contains no guest-memory accesses and is immutable before use.
pub fn prepare(vm: &Vm, address: u64) -> anyhow::Result<Access<'_>> {
  let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
  ensure!(page > 0, "Cannot determine PMU code page size");
  let mut memory = memory(vm, address, page as usize)?;
  let code = REGISTERS
    .into_iter()
    .flat_map(|register| {
      [0xd5300000, 0xd5100000]
        .into_iter()
        .flat_map(move |operation| {
          [
            operation | (u32::from(register & 0x7fff) << 5),
            0xd4000002,
            // a missed checkpoint must repeat this access, never enter a neighboring write.
            0x17fffffe,
            0xd4000022,
          ]
          .into_iter()
          .flat_map(u32::to_le_bytes)
        })
    })
    .collect::<Vec<_>>();
  write(&mut memory, 0, &code)?;
  // unexpected EL1 exceptions must return here, never enter the guest's handlers.
  for vector in 0..16 {
    write(
      &mut memory,
      0x800 + vector * 128,
      &0xd4000022u32.to_le_bytes(),
    )?;
  }
  protect(&memory, 5)?;
  ensure!(
    unsafe { libc::mprotect(memory.address.as_ptr().cast(), memory.size, libc::PROT_READ) } == 0,
    "Protect private PMU code: {}",
    std::io::Error::last_os_error()
  );
  Ok(Access { memory })
}

impl Access<'_> {
  pub fn token(&self) -> Token<'_> {
    Token {
      address: self.memory.guest,
      _owner: PhantomData,
    }
  }
}

impl Token<'_> {
  /// Every guest CPU must be held. Failure restoring the temporary context requires
  /// VM disposal. Event-counter configurations reject until their banks are covered.
  pub fn capture(&self, cpu: &mut Cpu<'_>) -> anyhow::Result<State> {
    context::run(cpu, self.address + 0x800, |cpu| {
      let features = self.access(cpu, 0xc028, None)?;
      ensure!(
        (features >> 8) & 15 == 1,
        "PMU feature configuration is not covered"
      );
      let mut values = Zeroizing::new([0; 8]);
      for (register, value) in SAVED.into_iter().zip(values.iter_mut()) {
        *value = self.access(cpu, register, None)?;
      }
      ensure!(
        (values[0] >> 11) & 31 == 0,
        "PMU event counters are not covered"
      );
      let saved = State {
        values: *values,
        features,
        affinity: super::state::read(cpu, 0xc005)?,
        frequency: self.access(cpu, 0xdf00, None)?,
      };
      saved.validate()?;
      Ok(saved)
    })
  }

  /// Restores the saved cycle value at this call's epoch. A later owner hold advances
  /// it again; runtime resume must coordinate its final restoration separately.
  /// Any failure requires discarding the destination VM before guest resumption.
  pub fn restore(&self, cpu: &mut Cpu<'_>, saved: &State) -> anyhow::Result<()> {
    saved.validate()?;
    context::run(cpu, self.address + 0x800, |cpu| {
      ensure!(
        super::state::read(cpu, 0xc005)? == saved.affinity,
        "PMU snapshot CPU affinity is incompatible"
      );
      ensure!(
        self.access(cpu, 0xc028, None)? == saved.features,
        "PMU snapshot features are incompatible"
      );
      ensure!(
        (self.access(cpu, 0xdce0, None)? >> 11) & 31 == 0,
        "PMU event counters are not covered"
      );
      ensure!(
        self.access(cpu, 0xdf00, None)? == saved.frequency,
        "PMU snapshot timer frequency is incompatible"
      );
      for (register, value) in [
        (0xdce2, u32::MAX as u64),
        (0xc4f2, u32::MAX as u64),
        (0xdce0, saved.values[0] & !7),
        (0xdce3, u32::MAX as u64),
        (0xdce5, saved.values[3]),
        (0xdcf0, saved.values[5]),
        (0xdf7f, saved.values[7]),
        (0xdce8, saved.values[4]),
        (0xdcf3, saved.values[2]),
        (0xdce0, saved.values[0] & !6),
        (0xdce1, saved.values[1]),
        (0xc4f1, saved.values[6]),
      ] {
        self.access(cpu, register, Some(value))?;
      }
      Ok(())
    })
  }

  fn access(&self, cpu: &mut Cpu<'_>, register: u16, value: Option<u64>) -> anyhow::Result<u64> {
    self.access_with(cpu, register, value, super::run)
  }

  fn access_with(
    &self,
    cpu: &mut Cpu<'_>,
    register: u16,
    value: Option<u64>,
    mut run: impl FnMut(&mut Cpu<'_>) -> anyhow::Result<super::Exit>,
  ) -> anyhow::Result<u64> {
    let index = REGISTERS
      .iter()
      .position(|&candidate| candidate == register)
      .context("Unknown PMU register")?;
    let address = self.address + index as u64 * 32 + if value.is_some() { 16 } else { 0 };
    for _ in 0..16 {
      // cancellation may leave PC at the completed checkpoint, beside another access.
      super::enter(cpu, address)?;
      if let Some(value) = value {
        super::set(cpu, 0, value)?;
      }
      match run(cpu)? {
        super::Exit::Canceled => continue,
        super::Exit::Exception { syndrome, .. }
          if crate::arm::decode(syndrome) == crate::arm::Trap::Hypercall(0)
            && super::get(cpu, 31)? == address + 8 =>
        {
          return super::get(cpu, 0);
        }
        exit => anyhow::bail!(
          "PMU access {register:#06x} did not reach checkpoint {:#x}: PC {:#x}, {exit:?}",
          address + 8,
          super::get(cpu, 31)?,
        ),
      }
    }
    anyhow::bail!("PMU access was repeatedly interrupted")
  }
}
