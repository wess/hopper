//! Each VM owns a process; each CPU stays on its creating thread.

mod ffi;
pub mod gic;

use anyhow::{bail, ensure, Context};
use std::marker::PhantomData;
use std::ptr::{null_mut, NonNull};
use std::rc::Rc;

pub struct Vm {
  _thread: PhantomData<Rc<()>>,
}

pub struct Memory<'a> {
  address: NonNull<u8>,
  guest: u64,
  size: usize,
  _vm: PhantomData<&'a Vm>,
}

pub struct Cpu<'a> {
  id: u64,
  exit: NonNull<ffi::Exit>,
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
  check(unsafe { ffi::hv_vm_create(null_mut()) }, "Create VM")?;
  Ok(Vm {
    _thread: PhantomData,
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

pub fn cpu(vm: &Vm) -> anyhow::Result<Cpu<'_>> {
  let mut id = 0;
  let mut exit = null_mut();
  check(
    unsafe { ffi::hv_vcpu_create(&mut id, &mut exit, null_mut()) },
    "Create CPU",
  )?;
  let Some(exit) = NonNull::new(exit) else {
    unsafe { ffi::hv_vcpu_destroy(id) };
    bail!("Hypervisor did not provide CPU exit information");
  };
  let _ = vm;
  Ok(Cpu {
    id,
    exit,
    _vm: PhantomData,
  })
}

pub fn set(cpu: &mut Cpu<'_>, register: u32, value: u64) -> anyhow::Result<()> {
  ensure!(register <= ffi::CPSR, "Invalid CPU register");
  check(
    unsafe { ffi::hv_vcpu_set_reg(cpu.id, register, value) },
    "Set CPU register",
  )
}

pub fn get(cpu: &Cpu<'_>, register: u32) -> anyhow::Result<u64> {
  ensure!(register <= ffi::CPSR, "Invalid CPU register");
  let mut value = 0;
  check(
    unsafe { ffi::hv_vcpu_get_reg(cpu.id, register, &mut value) },
    "Read CPU register",
  )?;
  Ok(value)
}

pub fn affinity(cpu: &mut Cpu<'_>, value: u64) -> anyhow::Result<()> {
  check(
    unsafe { ffi::hv_vcpu_set_sys_reg(cpu.id, 0xc005, value) },
    "Set CPU affinity",
  )
}

pub fn enter(cpu: &mut Cpu<'_>, address: u64) -> anyhow::Result<()> {
  set(cpu, ffi::PC, address)?;
  set(cpu, ffi::CPSR, 0x3c5)
}

pub fn run(cpu: &mut Cpu<'_>) -> anyhow::Result<Exit> {
  check(unsafe { ffi::hv_vcpu_run(cpu.id) }, "Run CPU")?;
  let exit = unsafe { *cpu.exit.as_ptr() };
  Ok(match exit.reason {
    0 => Exit::Canceled,
    1 => Exit::Exception {
      syndrome: exit.exception.syndrome,
      virtual_address: exit.exception.virtual_address,
      physical_address: exit.exception.physical_address,
    },
    2 => Exit::Timer,
    _ => Exit::Unknown,
  })
}

/// Request a native CPU exit at the deadline. The worker must return on cancellation.
pub fn bounded<T>(
  cpu: &mut Cpu<'_>,
  timeout: std::time::Duration,
  work: impl FnOnce(&mut Cpu<'_>) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
  std::thread::scope(|scope| {
    let (done, receiver) = std::sync::mpsc::sync_channel(1);
    let mut id = cpu.id;
    let timer = scope.spawn(move || -> anyhow::Result<bool> {
      if receiver.recv_timeout(timeout) == Err(std::sync::mpsc::RecvTimeoutError::Timeout) {
        check(
          unsafe { ffi::hv_vcpus_exit(&mut id, 1) },
          "Interrupt CPU at deadline",
        )?;
        return Ok(true);
      }
      Ok(false)
    });
    let result = work(cpu);
    let _ = done.send(());
    let expired = timer
      .join()
      .map_err(|_| anyhow::anyhow!("CPU deadline worker panicked"))??;
    if expired {
      return result
        .and_then(|_| anyhow::bail!("Guest execution deadline expired"))
        .context("Guest execution deadline expired");
    }
    result
  })
}

impl Drop for Cpu<'_> {
  fn drop(&mut self) {
    unsafe { ffi::hv_vcpu_destroy(self.id) };
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
