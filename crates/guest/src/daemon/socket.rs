use anyhow::{ensure, Context};
use std::{
  fs::OpenOptions,
  os::fd::{AsRawFd, FromRawFd, OwnedFd},
  os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt},
  sync::atomic::{AtomicBool, Ordering},
  time::{Duration, Instant},
};

pub(super) struct Socket(OwnedFd);

fn local_cid() -> anyhow::Result<u32> {
  let device = OpenOptions::new()
    .read(true)
    .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
    .open("/dev/vsock")?;
  let info = device.metadata()?;
  ensure!(
    info.file_type().is_char_device() && info.uid() == 0 && info.gid() == 0,
    "Guest vsock identity requires a root-owned character device"
  );
  let mut cid = libc::VMADDR_CID_ANY;
  // Linux UAPI IOCTL_VM_SOCKETS_GET_LOCAL_CID = _IO(7, 0xb9).
  ensure!(
    unsafe { libc::ioctl(device.as_raw_fd(), 0x7b9, &mut cid) } == 0,
    "Read guest vsock identity: {}",
    std::io::Error::last_os_error()
  );
  ensure!(
    cid > libc::VMADDR_CID_HOST && cid != libc::VMADDR_CID_ANY,
    "Guest transport cannot run on a host endpoint"
  );
  Ok(cid)
}

pub(super) fn check(stop: &AtomicBool, deadline: Option<Instant>) -> anyhow::Result<()> {
  ensure!(!stop.load(Ordering::Acquire), "Guest service was stopped");
  ensure!(
    deadline.is_none_or(|deadline| Instant::now() < deadline),
    "Guest transport timed out"
  );
  Ok(())
}

pub(super) fn connect(stop: &AtomicBool) -> anyhow::Result<Socket> {
  check(stop, None)?;
  let fd = unsafe {
    libc::socket(
      libc::AF_VSOCK,
      libc::SOCK_STREAM | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
      0,
    )
  };
  ensure!(
    fd >= 0,
    "Create guest socket: {}",
    std::io::Error::last_os_error()
  );
  let socket = Socket(unsafe { OwnedFd::from_raw_fd(fd) });
  let mut address: libc::sockaddr_vm = unsafe { std::mem::zeroed() };
  address.svm_family = libc::AF_VSOCK as _;
  address.svm_cid = libc::VMADDR_CID_HOST;
  address.svm_port = 6200;
  let result = unsafe {
    libc::connect(
      fd,
      (&address as *const libc::sockaddr_vm).cast(),
      std::mem::size_of_val(&address) as _,
    )
  };
  if result != 0 {
    let error = std::io::Error::last_os_error();
    ensure!(
      matches!(error.raw_os_error(), Some(libc::EINPROGRESS | libc::EAGAIN)),
      "Connect guest socket: {error}"
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    socket.wait(libc::POLLOUT, stop, Some(deadline))?;
    let mut error = 0i32;
    let mut length = std::mem::size_of_val(&error) as libc::socklen_t;
    ensure!(
      unsafe {
        libc::getsockopt(
          fd,
          libc::SOL_SOCKET,
          libc::SO_ERROR,
          (&mut error as *mut i32).cast(),
          &mut length,
        )
      } == 0
        && length as usize == std::mem::size_of_val(&error),
      "Read guest connection status"
    );
    ensure!(
      error == 0,
      "Connect guest socket: {}",
      std::io::Error::from_raw_os_error(error)
    );
  }
  let local_cid = local_cid()?;
  for (peer, expected_host) in [(true, true), (false, false)] {
    let mut address: libc::sockaddr_vm = unsafe { std::mem::zeroed() };
    let mut length = std::mem::size_of_val(&address) as libc::socklen_t;
    let result = unsafe {
      if peer {
        libc::getpeername(
          fd,
          (&mut address as *mut libc::sockaddr_vm).cast(),
          &mut length,
        )
      } else {
        libc::getsockname(
          fd,
          (&mut address as *mut libc::sockaddr_vm).cast(),
          &mut length,
        )
      }
    };
    ensure!(
      result == 0
        && length as usize == std::mem::size_of_val(&address)
        && address.svm_family == libc::AF_VSOCK as libc::sa_family_t,
      "Guest socket identity is unavailable"
    );
    ensure!(
      if expected_host {
        address.svm_cid == libc::VMADDR_CID_HOST && address.svm_port == 6200
      } else {
        address.svm_cid == local_cid || address.svm_cid == libc::VMADDR_CID_ANY
      },
      "Guest transport requires a guest-to-host connection (peer={peer}, cid={}, port={})",
      address.svm_cid,
      address.svm_port
    );
  }
  check(stop, None)?;
  Ok(socket)
}

impl Socket {
  fn wait(&self, event: i16, stop: &AtomicBool, deadline: Option<Instant>) -> anyhow::Result<()> {
    loop {
      check(stop, deadline)?;
      let mut poll = libc::pollfd {
        fd: self.0.as_raw_fd(),
        events: event,
        revents: 0,
      };
      let milliseconds = deadline.map_or(100, |deadline| {
        deadline
          .saturating_duration_since(Instant::now())
          .as_millis()
          .clamp(1, 100) as i32
      });
      let result = unsafe { libc::poll(&mut poll, 1, milliseconds) };
      if result < 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::Interrupted {
          continue;
        }
        return Err(error).context("Poll guest transport");
      }
      ensure!(
        poll.revents & libc::POLLNVAL == 0,
        "Guest socket is invalid"
      );
      if poll.revents & (event | libc::POLLERR | libc::POLLHUP) != 0 {
        check(stop, deadline)?;
        return Ok(());
      }
    }
  }

  pub(super) fn read(
    &self,
    bytes: &mut [u8],
    stop: &AtomicBool,
    deadline: Option<Instant>,
  ) -> anyhow::Result<usize> {
    ensure!(
      !bytes.is_empty() && bytes.len() <= model::guest::wire::CHUNK,
      "Guest socket read exceeds bounds"
    );
    loop {
      check(stop, deadline)?;
      let count = unsafe {
        libc::recv(
          self.0.as_raw_fd(),
          bytes.as_mut_ptr().cast(),
          bytes.len(),
          0,
        )
      };
      if count >= 0 {
        check(stop, deadline)?;
        return Ok(count as usize);
      }
      let error = std::io::Error::last_os_error();
      if error.kind() == std::io::ErrorKind::Interrupted {
        continue;
      }
      if error.kind() != std::io::ErrorKind::WouldBlock {
        return Err(error).context("Read guest transport");
      }
      self.wait(libc::POLLIN, stop, deadline)?;
    }
  }

  pub(super) fn exact(
    &self,
    mut bytes: &mut [u8],
    stop: &AtomicBool,
    deadline: Instant,
  ) -> anyhow::Result<()> {
    while !bytes.is_empty() {
      let count = self.read(bytes, stop, Some(deadline))?;
      ensure!(count != 0, "Guest connection closed during authentication");
      bytes = &mut bytes[count..];
    }
    Ok(())
  }

  pub(super) fn write(
    &self,
    mut bytes: &[u8],
    stop: &AtomicBool,
    deadline: Instant,
  ) -> anyhow::Result<()> {
    while !bytes.is_empty() {
      check(stop, Some(deadline))?;
      let count = unsafe {
        libc::send(
          self.0.as_raw_fd(),
          bytes.as_ptr().cast(),
          bytes.len().min(model::guest::wire::CHUNK),
          libc::MSG_NOSIGNAL,
        )
      };
      if count > 0 {
        check(stop, Some(deadline))?;
        bytes = &bytes[count as usize..];
        continue;
      }
      ensure!(count != 0, "Guest connection closed while writing");
      let error = std::io::Error::last_os_error();
      if error.kind() == std::io::ErrorKind::Interrupted {
        continue;
      }
      if error.kind() != std::io::ErrorKind::WouldBlock {
        return Err(error).context("Write guest transport");
      }
      self.wait(libc::POLLOUT, stop, Some(deadline))?;
    }
    Ok(())
  }
}
