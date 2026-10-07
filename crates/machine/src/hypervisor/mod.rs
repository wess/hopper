//! Each VM owns a process; each CPU stays on its creating thread.

mod config;
mod cpu;
mod exception;
mod ffi;
pub mod gic;
pub mod registers;
pub mod secondary;
mod watch;

pub use cpu::{affinity, cpu, create_cpu, enter, factory, get, run, set, Cpu, CpuFactory};
pub use exception::{fault, Fault};
pub use watch::{bounded, paced};

use anyhow::{bail, ensure, Context};
use std::cell::Cell;
use std::marker::PhantomData;
use std::ptr::{null_mut, NonNull};
use std::rc::Rc;

pub struct Vm {
  _thread: PhantomData<Rc<()>>,
  gic: Cell<bool>,
}

pub struct Memory<'a> {
  address: NonNull<u8>,
  guest: u64,
  size: usize,
  _vm: PhantomData<&'a Vm>,
}

#[derive(Clone, Copy, Debug)]
pub enum Exit {
  Canceled,
  Exception {
    syndrome: u64,
    virtual_address: u64,
    physical_address: u64,
  },
  Timer,
  Unknown,
}

fn check(status: i32, operation: &str) -> anyhow::Result<()> {
  if status != 0 {
    bail!("{operation}: Hypervisor error 0x{:08x}", status as u32);
  }
  Ok(())
}

pub fn create() -> anyhow::Result<Vm> {
  config::create()?;
  Ok(Vm {
    _thread: PhantomData,
    gic: Cell::new(false),
  })
}

pub fn memory(vm: &Vm, guest: u64, size: usize) -> anyhow::Result<Memory<'_>> {
  let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
  ensure!(page > 0, "Cannot determine host page size");
  let page = page as usize;
  ensure!(
    size > 0 && size.is_multiple_of(page),
    "Memory size must be page aligned"
  );
  ensure!(
    guest.is_multiple_of(page as u64),
    "Guest address must be page aligned"
  );
  ensure!(
    guest.checked_add(size as u64).is_some(),
    "Guest memory range overflows"
  );
  let address = unsafe {
    libc::mmap(
      null_mut(),
      size,
      libc::PROT_READ | libc::PROT_WRITE,
      libc::MAP_PRIVATE | libc::MAP_ANON,
      -1,
      0,
    )
  };
  if address == libc::MAP_FAILED {
    return Err(std::io::Error::last_os_error()).context("Allocate guest memory");
  }
  let mapped = check(
    unsafe { ffi::hv_vm_map(address, guest, size, 7) },
    "Map guest memory",
  );
  if let Err(error) = mapped {
    unsafe { libc::munmap(address, size) };
    return Err(error);
  }
  let _ = vm;
  Ok(Memory {
    address: NonNull::new(address.cast()).context("Guest memory allocation returned null")?,
    guest,
    size,
    _vm: PhantomData,
  })
}

pub fn write(memory: &mut Memory<'_>, offset: usize, bytes: &[u8]) -> anyhow::Result<()> {
  ensure!(
    offset
      .checked_add(bytes.len())
      .is_some_and(|end| end <= memory.size),
    "Write exceeds guest memory"
  );
  unsafe {
    std::ptr::copy_nonoverlapping(
      bytes.as_ptr(),
      memory.address.as_ptr().add(offset),
      bytes.len(),
    )
  };
  Ok(())
}

pub fn read(memory: &Memory<'_>, offset: usize, bytes: &mut [u8]) -> anyhow::Result<()> {
  ensure!(
    offset
      .checked_add(bytes.len())
      .is_some_and(|end| end <= memory.size),
    "Read exceeds guest memory"
  );
  unsafe {
    std::ptr::copy_nonoverlapping(
      memory.address.as_ptr().add(offset),
      bytes.as_mut_ptr(),
      bytes.len(),
    )
  };
  Ok(())
}

pub fn protect(memory: &Memory<'_>, flags: u64) -> anyhow::Result<()> {
  ensure!(flags & !7 == 0, "Invalid guest memory permissions");
  check(
    unsafe { ffi::hv_vm_protect(memory.guest, memory.size, flags) },
    "Protect guest memory",
  )
}

impl crate::dma::Memory for Memory<'_> {
  fn contains(&self, address: u64, length: usize) -> bool {
    address
      .checked_sub(self.guest)
      .and_then(|offset| offset.checked_add(length as u64))
      .is_some_and(|end| end <= self.size as u64)
  }

  fn read(&self, address: u64, bytes: &mut [u8]) -> anyhow::Result<()> {
    ensure!(
      self.contains(address, bytes.len()),
      "DMA read exceeds guest RAM"
    );
    read(self, (address - self.guest) as usize, bytes)
  }

  fn write(&mut self, address: u64, bytes: &[u8]) -> anyhow::Result<()> {
    ensure!(
      self.contains(address, bytes.len()),
      "DMA write exceeds guest RAM"
    );
    write(self, (address - self.guest) as usize, bytes)
  }
}

impl Drop for Memory<'_> {
  fn drop(&mut self) {
    // failed unmapping must not leave the guest pointing at freed host memory.
    if unsafe { ffi::hv_vm_unmap(self.guest, self.size) } == 0 {
      unsafe { libc::munmap(self.address.as_ptr().cast(), self.size) };
    }
  }
}

impl Drop for Vm {
  fn drop(&mut self) {
    unsafe { ffi::hv_vm_destroy() };
  }
}
