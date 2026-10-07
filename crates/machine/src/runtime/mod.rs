//! Native owner loop. Device slots are boot media, display, keyboard, tablet,
//! system disk, installer DVD and setup channel. Callbacks run on the VM owner thread.

use anyhow::ensure;
use std::time::Duration;
pub mod variables;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod native;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod trap;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub use native::{run, run_persistent, Stopped};

pub struct Boot {
  pub firmware: Vec<u8>,
  pub variables: Vec<u8>,
  pub memory: u64,
  pub cpus: u32,
  pub timeout: Duration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
  Continue,
  Stop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
  Stopped,
  Shutdown,
  Reset,
}

pub fn validate(boot: &Boot) -> anyhow::Result<()> {
  ensure!(
    !boot.firmware.is_empty() && boot.firmware.len() <= 0x4000000,
    "Firmware exceeds its flash bank"
  );
  ensure!(
    !boot.variables.is_empty() && boot.variables.len() <= 0x4000000,
    "Variable data exceeds its flash bank"
  );
  ensure!(
    (0x10000000..=0x1000000000).contains(&boot.memory) && boot.memory.is_multiple_of(0x4000),
    "Guest memory must be page aligned and between 256 MiB and 64 GiB"
  );
  ensure!(
    (1..=2).contains(&boot.cpus),
    "Native runtime supports one or two CPUs"
  );
  ensure!(
    boot.timeout >= Duration::from_millis(20),
    "Invalid native execution timeout"
  );
  Ok(())
}
