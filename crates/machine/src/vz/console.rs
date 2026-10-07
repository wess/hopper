use anyhow::ensure;
use objc2::AllocAnyThread;
use objc2_foundation::{NSArray, NSFileHandle};
use objc2_virtualization::{
  VZFileHandleSerialPortAttachment, VZSerialPortConfiguration,
  VZVirtioConsoleDeviceSerialPortConfiguration, VZVirtualMachineConfiguration,
};
use std::{fs::File, os::fd::AsRawFd};

pub(super) fn attach(config: &VZVirtualMachineConfiguration, output: &File) -> anyhow::Result<()> {
  unsafe {
    let fd = libc::fcntl(output.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 0);
    ensure!(fd >= 0, "Duplicate VZ console output descriptor");
    let handle =
      NSFileHandle::initWithFileDescriptor_closeOnDealloc(NSFileHandle::alloc(), fd, true);
    let attachment =
      VZFileHandleSerialPortAttachment::initWithFileHandleForReading_fileHandleForWriting(
        VZFileHandleSerialPortAttachment::alloc(),
        None,
        Some(&handle),
      );
    let console = VZVirtioConsoleDeviceSerialPortConfiguration::new();
    console.setAttachment(Some(&attachment));
    config.setSerialPorts(&NSArray::<VZSerialPortConfiguration>::from_slice(&[
      &console,
    ]));
  }
  Ok(())
}
