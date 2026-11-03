use super::{queue::Check, Linux, MainThreadMarker, Vm};
use anyhow::ensure;
use model::{
  guest::wire::auth::{Boot, Material, Secret},
  GuestOs,
};
use objc2::AllocAnyThread;
use objc2_foundation::{NSArray, NSFileHandle, NSString};
use objc2_virtualization::{
  VZConsoleDeviceConfiguration, VZFileHandleSerialPortAttachment,
  VZVirtioConsoleDeviceConfiguration, VZVirtioConsolePortConfiguration,
};
use std::{
  fs::File,
  io::{Read, Write},
  os::{
    fd::{AsRawFd, FromRawFd, IntoRawFd},
    unix::net::UnixStream,
  },
  time::{Duration, Instant},
};

use zeroize::Zeroizing;

pub const PORT: &str = "hopper.boot";

pub fn provision(
  main: MainThreadMarker,
  hardware: &Linux,
  id: &str,
  generation: u64,
  check: &Check,
) -> anyhow::Result<Vm> {
  check()?;
  let material = Material {
    boot: Boot {
      machine: id.into(),
      generation,
      guest: GuestOs::Linux,
    },
    key: Secret::generate()?,
  };
  let input = input(&material, check)?;
  let mut vm = create(main, hardware, input, check)?;
  vm.guest_material = Some(material);
  Ok(vm)
}

pub struct Input {
  file: File,
  boot: Boot,
  delivery: Option<Delivery>,
}

impl std::io::Read for Input {
  fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
    std::io::Read::read(&mut self.file, bytes)
  }
}

impl AsRawFd for Input {
  fn as_raw_fd(&self) -> std::os::fd::RawFd {
    self.file.as_raw_fd()
  }
}

pub(super) struct Delivery {
  socket: UnixStream,
  bytes: Zeroizing<Vec<u8>>,
  request: [u8; 4],
  received: usize,
  written: usize,
  deadline: Option<Instant>,
  check: Check,
}

impl Delivery {
  fn poll(&mut self, now: Instant) -> anyhow::Result<bool> {
    (self.check)()?;
    let deadline = self.deadline.get_or_insert(now + Duration::from_secs(180));
    ensure!(now < *deadline, "Private boot request timed out");
    if self.received < self.request.len() {
      match self.socket.read(&mut self.request[self.received..]) {
        Ok(0) => anyhow::bail!("Private boot request ended early"),
        Ok(count) => self.received += count,
        Err(error)
          if matches!(
            error.kind(),
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
          ) =>
        {
          return Ok(false)
        }
        Err(error) => return Err(error.into()),
      }
      (self.check)()?;
      if self.received < self.request.len() {
        return Ok(false);
      }
      ensure!(
        &self.request == Material::REQUEST,
        "Invalid private boot request"
      );
    }
    (self.check)()?;
    match self.socket.write(&self.bytes[self.written..]) {
      Ok(0) => anyhow::bail!("Private boot delivery ended early"),
      Ok(count) => self.written += count,
      Err(error)
        if matches!(
          error.kind(),
          std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
        ) =>
      {
        return Ok(false)
      }
      Err(error) => return Err(error.into()),
    }
    (self.check)()?;
    Ok(self.written == self.bytes.len())
  }
}

impl Input {
  /// drive one bounded private exchange; hardware owners call this on their VM queue.
  pub fn poll(&mut self, now: Instant) -> anyhow::Result<bool> {
    let Some(delivery) = self.delivery.as_mut() else {
      return Ok(true);
    };
    let result = delivery.poll(now);
    if !matches!(result, Ok(false)) {
      self.delivery.take();
    }
    result
  }
}

/// a single-use private duplex stream; no filesystem copy of the key is created.
pub fn input(material: &Material, check: &Check) -> anyhow::Result<Input> {
  check()?;
  ensure!(
    material.boot.guest == GuestOs::Linux,
    "Boot channel requires Linux material"
  );
  let (guest, socket) = UnixStream::pair()?;
  socket.set_nonblocking(true)?;
  let mut bytes = Zeroizing::new(Vec::with_capacity(Material::LENGTH));
  material.write(&mut *bytes)?;
  check()?;
  Ok(Input {
    file: unsafe { File::from_raw_fd(guest.into_raw_fd()) },
    boot: material.boot.clone(),
    delivery: Some(Delivery {
      socket,
      bytes,
      request: [0; 4],
      received: 0,
      written: 0,
      deadline: None,
      check: check.clone(),
    }),
  })
}

pub(super) fn active(vm: &Vm) -> bool {
  vm.started.get() && vm.boot_delivery.borrow().is_some()
}

pub(super) fn tick(vm: &Vm) {
  if !active(vm) {
    return;
  }
  let mut delivery = vm.boot_delivery.borrow_mut();
  if vm.stop_requested.get() || super::state(vm) == super::VZVirtualMachineState::Stopped {
    delivery.take();
    return;
  }
  let now = Instant::now();
  if let Some(pending) = delivery.as_mut() {
    if (pending.check)().is_err() || pending.deadline.is_some_and(|deadline| now >= deadline) {
      delivery.take();
      return;
    }
    if super::state(vm) != super::VZVirtualMachineState::Running {
      return;
    }
    match pending.poll(now) {
      Ok(false) => {}
      Ok(true) | Err(_) => {
        delivery.take();
      }
    }
  }
}

/// the private named port is separate from the serial output used by diagnostics.
pub fn create(
  main: MainThreadMarker,
  boot: &Linux,
  mut input: Input,
  check: &Check,
) -> anyhow::Result<Vm> {
  check()?;
  let config = super::config::linux(boot)?;
  unsafe {
    let descriptor = libc::fcntl(input.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 0);
    ensure!(descriptor >= 0, "Duplicate private boot input descriptor");
    let read =
      NSFileHandle::initWithFileDescriptor_closeOnDealloc(NSFileHandle::alloc(), descriptor, true);
    let descriptor = libc::fcntl(input.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 0);
    ensure!(descriptor >= 0, "Duplicate private boot sink descriptor");
    let write =
      NSFileHandle::initWithFileDescriptor_closeOnDealloc(NSFileHandle::alloc(), descriptor, true);
    let attachment =
      VZFileHandleSerialPortAttachment::initWithFileHandleForReading_fileHandleForWriting(
        VZFileHandleSerialPortAttachment::alloc(),
        Some(&read),
        Some(&write),
      );
    let port = VZVirtioConsolePortConfiguration::new();
    port.setName(Some(&NSString::from_str(PORT)));
    port.setIsConsole(false);
    port.setAttachment(Some(&attachment));
    let device = VZVirtioConsoleDeviceConfiguration::new();
    device.ports().setMaximumPortCount(1);
    device.ports().setObject_atIndexedSubscript(Some(&port), 0);
    config.setConsoleDevices(&NSArray::<VZConsoleDeviceConfiguration>::from_slice(&[
      &device,
    ]));
    config
      .validateWithError()
      .map_err(|error| anyhow::anyhow!("Invalid private boot channel: {error}"))?;
  }
  check()?;
  let mut vm = super::configured(main, &config, false, &boot.shares);
  vm.guest_boot = Some(input.boot);
  vm.boot_delivery.replace(input.delivery.take());
  Ok(vm)
}
