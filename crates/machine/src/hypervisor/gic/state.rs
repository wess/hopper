//! opaque, SDK-versioned interrupt state; CPU interface registers are saved separately.

use super::Gic;
use crate::hypervisor::{check, ffi};
use anyhow::{ensure, Context};
use std::ffi::c_void;
use std::ptr::NonNull;
use zeroize::Zeroizing;

pub mod wire;

const LIMIT: usize = 16 * 1024 * 1024;

pub struct State(Zeroizing<Vec<u8>>);

struct Captured(NonNull<c_void>);

impl Drop for Captured {
  fn drop(&mut self) {
    unsafe { ffi::os_release(self.0.as_ptr()) };
  }
}

/// All VM CPUs must be held outside guest execution throughout capture.
pub fn capture(_gic: &Gic<'_>) -> anyhow::Result<State> {
  let state = Captured(
    NonNull::new(unsafe { ffi::hv_gic_state_create() })
      .context("Capture interrupt controller state")?,
  );
  let mut size = 0;
  check(
    unsafe { ffi::hv_gic_state_get_size(state.0.as_ptr(), &mut size) },
    "Read interrupt state size",
  )?;
  ensure!(
    (1..=LIMIT).contains(&size),
    "Interrupt state exceeds its bound"
  );
  let mut bytes = Zeroizing::new(vec![0; size]);
  check(
    unsafe { ffi::hv_gic_state_get_data(state.0.as_ptr(), bytes.as_mut_ptr().cast()) },
    "Read interrupt state",
  )?;
  Ok(State(bytes))
}

impl State {
  pub fn from_bytes(bytes: &[u8]) -> anyhow::Result<Self> {
    ensure!(
      (1..=LIMIT).contains(&bytes.len()),
      "Interrupt state exceeds its bound"
    );
    Ok(Self(Zeroizing::new(bytes.to_vec())))
  }

  pub fn bytes(&self) -> &[u8] {
    &self.0
  }
}

/// Restore only after recreating every CPU and before running any of them.
/// The SDK rejects incompatible state. CPU registers and other devices must match.
pub fn restore(_gic: &Gic<'_>, state: &State) -> anyhow::Result<()> {
  check(
    unsafe { ffi::hv_gic_set_state(state.0.as_ptr().cast(), state.0.len()) },
    "Restore interrupt state",
  )
}
