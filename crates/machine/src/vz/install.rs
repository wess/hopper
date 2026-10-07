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
  pending: Pending,
  _vm: &'vm mut Vm,
}

pub(crate) struct Pending {
  installer: Retained<VZMacOSInstaller>,
  receive: mpsc::Receiver<anyhow::Result<()>>,
  done: Rc<Cell<bool>>,
  cancelled: Rc<Cell<bool>>,
}

pub fn start<'vm>(vm: &'vm mut Vm, path: &Path) -> anyhow::Result<Installation<'vm>> {
  let pending = start_checked(vm, path, None, None)?;
  Ok(Installation { pending, _vm: vm })
}

pub(crate) fn start_checked(
  vm: &Vm,
  path: &Path,
  check: Option<super::queue::Check>,
  commit: Option<super::queue::Check>,
) -> anyhow::Result<Pending> {
  if let Some(check) = &check {
    check()?;
  }
  ensure!(vm.mac, "macOS installation requires a macOS platform");
  ensure!(!vm.installing.get(), "macOS installation is already active");
  ensure!(
    !vm.mac_ready.get(),
    "macOS installation has already completed"
  );
  ensure!(
    super::state(vm) == VZVirtualMachineState::Stopped,
    "macOS installation requires a stopped VM"
  );
  for directory in &*vm.shares {
    directory.validate()?;
  }
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
  if let Some(check) = &check {
    check()?;
  }
  let (send, receive) = mpsc::sync_channel(1);
  let busy = vm.installing.clone();
  let ready = vm.mac_ready.clone();
  let done = Rc::new(Cell::new(false));
  let completed = done.clone();
  let cancelled = Rc::new(Cell::new(false));
  let requested = cancelled.clone();
  let stopped = vm.stop_requested.clone();
  // cancellation is asynchronous; the callback owns hardware until it acknowledges completion.
  let held = Rc::new(RefCell::new(Some((
    installer.clone(),
    Some(super::hold(vm)),
    check,
    commit,
  ))));
  let completion = RcBlock::new(move |error: *mut NSError| {
    let authorized = held
      .borrow()
      .as_ref()
      .and_then(|(_, _, check, _)| check.as_ref())
      .map_or(Ok(()), |check| check());
    let result = if requested.get() || stopped.get() {
      Err(anyhow::anyhow!("macOS installation was cancelled"))
    } else if let Err(error) = authorized {
      Err(error)
    } else if error.is_null() {
      let result = (|| {
        let retained = held.borrow();
        let (_, _, check, commit) = retained
          .as_ref()
          .context("Installer completion lost ownership")?;
        if let Some(commit) = commit {
          commit()?;
        }
        if let Some(check) = check {
          check()?;
        }
        ensure!(
          !requested.get() && !stopped.get(),
          "macOS installation was cancelled"
        );
        ready.set(true);
        Ok(())
      })();
      result
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
  Ok(Pending {
    installer,
    receive,
    done,
    cancelled,
  })
}

pub fn fraction(installation: &Installation<'_>) -> f64 {
  fraction_pending(&installation.pending)
}

pub(crate) fn fraction_pending(installation: &Pending) -> f64 {
  let fraction = unsafe { installation.installer.progress() }.fractionCompleted();
  if fraction.is_finite() {
    fraction.clamp(0.0, 1.0)
  } else {
    0.0
  }
}

pub fn cancel(installation: &Installation<'_>) {
  cancel_pending(&installation.pending);
}

pub(crate) fn cancel_pending(installation: &Pending) {
  if !installation.done.get() {
    installation.cancelled.set(true);
    unsafe { installation.installer.progress() }.cancel();
  }
}

pub fn poll(installation: &Installation<'_>) -> anyhow::Result<Option<anyhow::Result<()>>> {
  poll_pending(&installation.pending)
}

pub(crate) fn poll_pending(installation: &Pending) -> anyhow::Result<Option<anyhow::Result<()>>> {
  match installation.receive.try_recv() {
    Ok(result) => Ok(Some(result)),
    Err(mpsc::TryRecvError::Empty) => Ok(None),
    Err(error) => Err(error).context("macOS installer completion ended without a result"),
  }
}

impl Drop for Pending {
  fn drop(&mut self) {
    cancel_pending(self);
  }
}
