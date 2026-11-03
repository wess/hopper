//! Read-only copy-on-write RAM ownership, independent of native hardware.

use super::Memory;
use anyhow::ensure;
mod restore;
pub use restore::restore;
pub mod wire;

pub struct Capture {
  address: u64,
  size: usize,
  guest: u64,
}

extern "C" {
  static mach_task_self_: u32;
  fn mach_vm_remap(
    task: u32,
    address: *mut u64,
    size: u64,
    mask: u64,
    flags: i32,
    source_task: u32,
    source: u64,
    copy: i32,
    current: *mut i32,
    maximum: *mut i32,
    inheritance: u32,
  ) -> i32;
  fn mach_vm_protect(task: u32, address: u64, size: u64, maximum: i32, protection: i32) -> i32;
  fn mach_vm_deallocate(task: u32, address: u64, size: u64) -> i32;
}

/// All guest CPUs and device writes must be held at the same checkpoint boundary.
/// The returned mapping remains immutable through later guest/DMA writes and VM teardown.
pub fn capture(memory: &Memory<'_>) -> anyhow::Result<Capture> {
  let mut address = 0;
  let mut current = 0;
  let mut maximum = 0;
  let task = unsafe { mach_task_self_ };
  let status = unsafe {
    mach_vm_remap(
      task,
      &mut address,
      memory.size as u64,
      0,
      1, // VM_FLAGS_ANYWHERE
      task,
      memory.address.as_ptr() as u64,
      1,
      &mut current,
      &mut maximum,
      2, // VM_INHERIT_NONE
    )
  };
  ensure!(status == 0, "Capture guest RAM: Mach error {status}");
  let captured = Capture {
    address,
    size: memory.size,
    guest: memory.guest,
  };
  let status = unsafe { mach_vm_protect(task, address, memory.size as u64, 1, 1) };
  ensure!(status == 0, "Protect captured RAM: Mach error {status}");
  Ok(captured)
}

impl Capture {
  pub fn guest(&self) -> u64 {
    self.guest
  }

  pub fn size(&self) -> usize {
    self.size
  }

  pub fn read(&self, offset: usize, bytes: &mut [u8]) -> anyhow::Result<()> {
    ensure!(bytes.len() <= 64 * 1024, "Captured RAM reads exceed 64 KiB");
    ensure!(
      offset
        .checked_add(bytes.len())
        .is_some_and(|end| end <= self.size),
      "Read exceeds captured RAM"
    );
    unsafe {
      std::ptr::copy_nonoverlapping(
        (self.address as *const u8).add(offset),
        bytes.as_mut_ptr(),
        bytes.len(),
      );
    }
    Ok(())
  }
}

impl Drop for Capture {
  fn drop(&mut self) {
    // mappings are private to this owner; no VM or source allocation is retained.
    unsafe { mach_vm_deallocate(mach_task_self_, self.address, self.size as u64) };
  }
}
