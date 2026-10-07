use super::{queue::Check, VZVirtualMachineState, Vm};
use anyhow::{ensure, Context};
use objc2::{
  define_class, msg_send,
  rc::Retained,
  runtime::{Bool, ProtocolObject},
  DefinedClass,
};
use objc2::{MainThreadOnly, Message};
use objc2_foundation::{NSObject, NSObjectProtocol};
use objc2_virtualization::{
  VZVirtioSocketConnection, VZVirtioSocketDevice, VZVirtioSocketListener,
  VZVirtioSocketListenerDelegate, VZVirtualMachine,
};
use std::{cell::RefCell, rc::Rc};

pub const PORT: u32 = 6200;
const LIMIT: usize = 64 << 10;

struct State {
  pending: RefCell<Option<Retained<VZVirtioSocketConnection>>>,
  machine: Retained<VZVirtualMachine>,
  stopped: Rc<std::cell::Cell<bool>>,
  check: Check,
}

impl State {
  fn authorized(&self) -> anyhow::Result<()> {
    (self.check)()?;
    ensure!(!self.stopped.get(), "Guest transport was stopped");
    ensure!(
      unsafe { self.machine.state() } == VZVirtualMachineState::Running,
      "Guest transport requires running hardware"
    );
    Ok(())
  }
}

define_class!(
  #[unsafe(super = NSObject)]
  #[thread_kind = MainThreadOnly]
  #[ivars = Rc<State>]
  struct Delegate;

  unsafe impl NSObjectProtocol for Delegate {}
  unsafe impl VZVirtioSocketListenerDelegate for Delegate {
    #[unsafe(method(listener:shouldAcceptNewConnection:fromSocketDevice:))]
    fn accept(
      &self,
      _listener: &VZVirtioSocketListener,
      connection: &VZVirtioSocketConnection,
      _device: &VZVirtioSocketDevice,
    ) -> Bool {
      let state = self.ivars();
      if state.authorized().is_err() || state.pending.borrow().is_some() {
        return Bool::NO;
      }
      let fd = unsafe { connection.fileDescriptor() };
      let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
      let descriptor_flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
      let enabled: libc::c_int = 1;
      if flags < 0 || descriptor_flags < 0
        || unsafe { libc::fcntl(fd, libc::F_SETFD, descriptor_flags | libc::FD_CLOEXEC) } < 0
        || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
        || unsafe {
          libc::setsockopt(fd, libc::SOL_SOCKET, libc::SO_NOSIGPIPE,
            (&enabled as *const libc::c_int).cast(), std::mem::size_of_val(&enabled) as _)
        } < 0
      {
        return Bool::NO;
      }
      if state.authorized().is_err() { return Bool::NO; }
      *state.pending.borrow_mut() = Some(connection.retain());
      Bool::YES
    }
  }
);

/// a guest connection and its policy stay on the owned VM queue.
///
/// ```compile_fail
/// fn move_socket(socket: machine::vz::socket::Listener) {
///   std::thread::spawn(move || drop(socket));
/// }
/// ```
pub struct Listener {
  listener: Retained<VZVirtioSocketListener>,
  _delegate: Retained<Delegate>,
  device: Retained<VZVirtioSocketDevice>,
  state: Rc<State>,
  listening: Rc<std::cell::Cell<bool>>,
  _ownership: std::sync::Arc<dyn Send + Sync>,
}

pub fn listen(vm: &Vm, check: Check) -> anyhow::Result<Listener> {
  check()?;
  ensure!(!vm.mac, "macOS guest transport is not configured");
  ensure!(
    !vm.listening.get(),
    "Guest transport already has a listener"
  );
  ensure!(
    !vm.installing.get() && !vm.stop_requested.get(),
    "Guest transport is unavailable"
  );
  let devices = unsafe { vm.machine.socketDevices() };
  ensure!(
    devices.len() == 1,
    "Guest transport requires one socket device"
  );
  let device = devices
    .objectAtIndex(0)
    .downcast::<VZVirtioSocketDevice>()
    .map_err(|_| anyhow::anyhow!("Guest transport requires virtio sockets"))?;
  let state = Rc::new(State {
    pending: RefCell::new(None),
    machine: vm.machine.clone(),
    stopped: vm.stop_requested.clone(),
    check,
  });
  let delegate = Delegate::alloc(vm._main).set_ivars(state.clone());
  let delegate: Retained<Delegate> = unsafe { msg_send![super(delegate), init] };
  let listener = unsafe { VZVirtioSocketListener::new() };
  unsafe {
    listener.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    device.setSocketListener_forPort(&listener, PORT);
  }
  vm.listening.set(true);
  Ok(Listener {
    listener,
    _delegate: delegate,
    device,
    state,
    listening: vm.listening.clone(),
    _ownership: super::hold(vm),
  })
}

impl Listener {
  pub fn connected(&self) -> anyhow::Result<bool> {
    self.check()?;
    Ok(self.state.pending.borrow().is_some())
  }

  fn check(&self) -> anyhow::Result<()> {
    let result = self.state.authorized();
    if result.is_err() {
      self.disconnect();
    }
    result
  }

  pub fn disconnect(&self) {
    if let Some(connection) = self.state.pending.borrow_mut().take() {
      unsafe { connection.close() };
    }
  }

  pub fn read(&self, bytes: &mut [u8]) -> anyhow::Result<Option<usize>> {
    self.check()?;
    ensure!(
      !bytes.is_empty() && bytes.len() <= LIMIT,
      "Guest read exceeds bounds"
    );
    let pending = self.state.pending.borrow();
    let connection = pending
      .as_ref()
      .context("Guest transport is not connected")?;
    let count = result(unsafe {
      libc::read(
        connection.fileDescriptor(),
        bytes.as_mut_ptr().cast(),
        bytes.len(),
      )
    });
    drop(pending);
    if let Err(error) = self.check() {
      bytes.fill(0);
      return Err(error);
    }
    if matches!(count, Ok(Some(0)) | Err(_)) {
      self.disconnect();
    }
    count
  }

  pub fn write(&self, bytes: &[u8]) -> anyhow::Result<Option<usize>> {
    self.check()?;
    ensure!(
      !bytes.is_empty() && bytes.len() <= LIMIT,
      "Guest write exceeds bounds"
    );
    let pending = self.state.pending.borrow();
    let connection = pending
      .as_ref()
      .context("Guest transport is not connected")?;
    let count = result(unsafe {
      libc::write(
        connection.fileDescriptor(),
        bytes.as_ptr().cast(),
        bytes.len(),
      )
    });
    drop(pending);
    self.check()?;
    if count.is_err() {
      self.disconnect();
    }
    count
  }
}

fn result(count: isize) -> anyhow::Result<Option<usize>> {
  if count >= 0 {
    return Ok(Some(count as usize));
  }
  let error = std::io::Error::last_os_error();
  if matches!(
    error.kind(),
    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
  ) {
    return Ok(None);
  }
  Err(error).context("Guest socket I/O failed")
}

impl Drop for Listener {
  fn drop(&mut self) {
    unsafe {
      self.device.removeSocketListenerForPort(PORT);
      self.listener.setDelegate(None);
      if let Some(connection) = self.state.pending.borrow_mut().take() {
        connection.close();
      }
    }
    self.listening.set(false);
  }
}
