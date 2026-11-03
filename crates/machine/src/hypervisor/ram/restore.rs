use super::Capture;
use crate::hypervisor::{check, ffi, Memory, Vm};
use anyhow::{ensure, Context};
use std::{marker::PhantomData, ptr::NonNull};

extern "C" {
  fn mach_vm_copy(task: u32, source: u64, size: u64, destination: u64) -> i32;
}

struct Mapping {
  address: NonNull<libc::c_void>,
  size: usize,
}

impl Drop for Mapping {
  fn drop(&mut self) {
    unsafe { libc::munmap(self.address.as_ptr(), self.size) };
  }
}

/// Rebuild an independent writable COW view before any destination CPU executes.
/// Hypervisor mapping rejects occupied guest ranges; failure releases the new host mapping.
/// The capture keeps its maximum read-only protection and can outlive either VM.
pub fn restore<'a>(vm: &'a Vm, saved: &Capture) -> anyhow::Result<Memory<'a>> {
  crate::hypervisor::validate_memory(vm, saved.guest, saved.size)?;
  let address = unsafe {
    libc::mmap(
      std::ptr::null_mut(),
      saved.size,
      libc::PROT_READ | libc::PROT_WRITE,
      libc::MAP_PRIVATE | libc::MAP_ANON,
      -1,
      0,
    )
  };
  ensure!(
    address != libc::MAP_FAILED,
    "Allocate restored guest memory: {}",
    std::io::Error::last_os_error()
  );
  let Some(address) = NonNull::new(address) else {
    unsafe { libc::munmap(address, saved.size) };
    anyhow::bail!("Restored guest memory returned null");
  };
  let mapping = Mapping {
    address,
    size: saved.size,
  };
  let status = unsafe {
    mach_vm_copy(
      super::mach_task_self_,
      saved.address,
      saved.size as u64,
      address.as_ptr() as u64,
    )
  };
  ensure!(status == 0, "Restore COW guest memory: Mach error {status}");
  check(
    unsafe { ffi::hv_vm_map(address.as_ptr(), saved.guest, saved.size, 7) },
    "Map restored COW guest memory",
  )
  .context("Admit restored guest memory")?;
  let _ = vm;
  std::mem::forget(mapping);
  Ok(Memory {
    address: address.cast(),
    guest: saved.guest,
    size: saved.size,
    _vm: PhantomData,
  })
}
