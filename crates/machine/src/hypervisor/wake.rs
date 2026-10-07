use super::{check, ffi, Cpu};
use std::sync::{Arc, Mutex};

/// A thread-safe exit request; it carries no guest register or memory access.
#[derive(Clone)]
pub struct Wake(Arc<Mutex<Option<u64>>>);

pub fn wake(cpu: &Cpu<'_>) -> Wake {
  cpu.wake.clone()
}

pub fn request_exit(wake: &Wake) -> anyhow::Result<bool> {
  let active = wake
    .0
    .lock()
    .map_err(|_| anyhow::anyhow!("CPU wake lock poisoned"))?;
  let Some(mut id) = *active else {
    return Ok(false);
  };
  check(unsafe { ffi::hv_vcpus_exit(&mut id, 1) }, "Wake CPU owner")?;
  Ok(true)
}

pub(super) fn create(id: u64) -> Wake {
  Wake(Arc::new(Mutex::new(Some(id))))
}

pub(super) fn destroy(wake: &Wake) {
  let mut active = wake.0.lock().unwrap_or_else(|error| error.into_inner());
  // serialize cancellation with destruction so retained handles cannot target reused ids.
  if let Some(id) = active.take() {
    unsafe { ffi::hv_vcpu_destroy(id) };
  }
}
