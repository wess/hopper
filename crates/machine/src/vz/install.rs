use super::{config, Vm};
use anyhow::{ensure, Context};
use block2::RcBlock;
use objc2::{rc::Retained, AllocAnyThread};
use objc2_foundation::NSError;
use objc2_virtualization::{VZMacOSInstaller, VZVirtualMachineState};
use std::{
  cell::{Cell, RefCell},
  path::Path,
  rc::Rc,
  sync::mpsc,
};

/// installation holds exclusive VM access and remains on its queue.
///
/// ```compile_fail
/// fn move_installation(installation: machine::vz::install::Installation<'static>) {
///   std::thread::spawn(move || drop(installation));
/// }
/// ```
/// ```compile_fail
/// fn control_during_install(vm: &mut machine::vz::Vm, media: &std::path::Path) {
///   let installation = machine::vz::install::start(vm, media).unwrap();
///   machine::vz::transition(vm, machine::vz::Action::Stop).unwrap();
///   drop(installation);
/// }
/// ```
pub struct Installation<'vm> {
  installer: Retained<VZMacOSInstaller>,
  receive: mpsc::Receiver<anyhow::Result<()>>,
  done: Rc<Cell<bool>>,
  _vm: &'vm mut Vm,
}

pub fn start<'vm>(vm: &'vm mut Vm, path: &Path) -> anyhow::Result<Installation<'vm>> {
  ensure!(vm.mac, "macOS installation requires a macOS platform");
  ensure!(!vm.installing.get(), "macOS installation is already active");
  ensure!(
    super::state(vm) == VZVirtualMachineState::Stopped,
    "macOS installation requires a stopped VM"
  );
  let url = config::file(path)?;
  let size = std::fs::metadata(path)?.len();
  ensure!(
    (1..=64 << 30).contains(&size),
    "Restore file size exceeds bounds"
  );
  let installer = unsafe {
    VZMacOSInstaller::initWithVirtualMachine_restoreImageURL(
      VZMacOSInstaller::alloc(),
      &vm.machine,
      &url,
    )
  };
  let (send, receive) = mpsc::sync_channel(1);
  let busy = vm.installing.clone();
  let done = Rc::new(Cell::new(false));
  let completed = done.clone();
  // cancellation is asynchronous; the callback owns hardware until it acknowledges completion.
  let held = Rc::new(RefCell::new(Some(installer.clone())));
  let completion = RcBlock::new(move |error: *mut NSError| {
    let result = if error.is_null() {
      Ok(())
    } else {
      let message: String = unsafe { &*error }.to_string().chars().take(512).collect();
      Err(anyhow::anyhow!("macOS installation failed: {message}"))
    };
    busy.set(false);
    completed.set(true);
    let _ = send.try_send(result);
    held.borrow_mut().take();
  });
  vm.installing.set(true);
  unsafe {
    installer.installWithCompletionHandler(&completion);
  }
  Ok(Installation {
    installer,
    receive,
    done,
    _vm: vm,
  })
}

pub fn fraction(installation: &Installation<'_>) -> f64 {
  let fraction = unsafe { installation.installer.progress() }.fractionCompleted();
  if fraction.is_finite() {
    fraction.clamp(0.0, 1.0)
  } else {
    0.0
  }
}

pub fn cancel(installation: &Installation<'_>) {
  if !installation.done.get() {
    unsafe { installation.installer.progress() }.cancel();
  }
}

pub fn poll(installation: &Installation<'_>) -> anyhow::Result<Option<anyhow::Result<()>>> {
  match installation.receive.try_recv() {
    Ok(result) => Ok(Some(result)),
    Err(mpsc::TryRecvError::Empty) => Ok(None),
    Err(error) => Err(error).context("macOS installer completion ended without a result"),
  }
}

impl Drop for Installation<'_> {
  fn drop(&mut self) {
    cancel(self);
  }
}
