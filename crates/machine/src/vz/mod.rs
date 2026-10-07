mod auxiliary;
mod config;
mod console;
mod display;
pub mod install;
mod kernel;
pub mod mac;
pub mod network;
pub mod queue;
pub mod restore;

use anyhow::{ensure, Context};
use block2::RcBlock;
pub use config::{create_variables, identity};
pub use display::Display;
pub use objc2::MainThreadMarker;
use objc2::{rc::Retained, AllocAnyThread};
use objc2_foundation::NSError;
use objc2_virtualization::VZVirtualMachine;
pub use objc2_virtualization::VZVirtualMachineState;
use std::{
  cell::{Cell, RefCell},
  path::PathBuf,
  rc::Rc,
  sync::mpsc,
};

pub struct Linux {
  pub cpus: usize,
  pub memory: u64,
  pub width: usize,
  pub height: usize,
  pub identity: Vec<u8>,
  pub boot: Boot,
  pub disk: PathBuf,
  pub installer: Option<PathBuf>,
  pub network: Option<network::Mode>,
  pub console: Option<std::fs::File>,
}

pub enum Boot {
  Efi {
    variables: PathBuf,
  },
  Kernel {
    kernel: PathBuf,
    initramfs: Option<PathBuf>,
    command_line: String,
  },
}

/// main-queue ownership cannot cross into the async runtime.
///
/// ```compile_fail
/// fn move_vm(vm: machine::vz::Vm) {
///   std::thread::spawn(move || drop(vm));
/// }
/// ```
pub struct Vm {
  machine: Retained<VZVirtualMachine>,
  _main: MainThreadMarker,
  mac: bool,
  installing: Rc<Cell<bool>>,
  ownership: Option<std::sync::Arc<dyn Send + Sync>>,
  displaying: Rc<Cell<bool>>,
}

#[derive(Clone, Copy)]
pub enum Action {
  Start,
  Pause,
  Resume,
  Stop,
}

pub struct Pending(mpsc::Receiver<anyhow::Result<()>>);

pub fn create(main: MainThreadMarker, boot: &Linux) -> anyhow::Result<Vm> {
  let config = config::linux(boot)?;
  Ok(configured(main, &config, false))
}

pub fn create_mac(main: MainThreadMarker, boot: &mac::Mac) -> anyhow::Result<Vm> {
  let config = mac::config(boot)?;
  Ok(configured(main, &config, true))
}

fn configured(
  main: MainThreadMarker,
  config: &objc2_virtualization::VZVirtualMachineConfiguration,
  mac: bool,
) -> Vm {
  let machine =
    unsafe { VZVirtualMachine::initWithConfiguration(VZVirtualMachine::alloc(), config) };
  Vm {
    machine,
    _main: main,
    mac,
    installing: Rc::new(Cell::new(false)),
    ownership: None,
    displaying: Rc::new(Cell::new(false)),
  }
}

pub fn retain(vm: &mut Vm, ownership: std::sync::Arc<dyn Send + Sync>) -> anyhow::Result<()> {
  ensure!(
    state(vm) == VZVirtualMachineState::Stopped && !vm.installing.get(),
    "Bind runtime ownership before starting the VM"
  );
  ensure!(
    vm.ownership.is_none(),
    "VZ runtime ownership is already bound"
  );
  vm.ownership = Some(ownership);
  Ok(())
}

pub fn state(vm: &Vm) -> VZVirtualMachineState {
  unsafe { vm.machine.state() }
}

pub fn transition(vm: &Vm, action: Action) -> anyhow::Result<Pending> {
  scoped_transition(vm, action, None)
}

fn scoped_transition(
  vm: &Vm,
  action: Action,
  check: Option<queue::Check>,
) -> anyhow::Result<Pending> {
  if let Some(check) = &check {
    check()?;
  }
  ensure!(
    !vm.installing.get(),
    "macOS installation owns the VM lifecycle"
  );
  let (send, receive) = mpsc::sync_channel(1);
  let held = Rc::new(RefCell::new(Some((
    vm.machine.clone(),
    check,
    vm.ownership.clone(),
  ))));
  let completion = RcBlock::new(move |error: *mut NSError| {
    let result = if error.is_null() {
      Ok(())
    } else {
      let message: String = unsafe { &*error }.to_string().chars().take(512).collect();
      Err(anyhow::anyhow!("VZ transition failed: {message}"))
    };
    let ownership = held.borrow_mut().take();
    let result = if let Some((_, Some(check), _)) = &ownership {
      check().and(result)
    } else {
      result
    };
    let _ = send.try_send(result);
  });
  unsafe {
    let permitted = match action {
      Action::Start => vm.machine.canStart(),
      Action::Pause => vm.machine.canPause(),
      Action::Resume => vm.machine.canResume(),
      Action::Stop => vm.machine.canStop(),
    };
    ensure!(permitted, "VZ state does not permit this transition");
    match action {
      Action::Start => vm.machine.startWithCompletionHandler(&completion),
      Action::Pause => vm.machine.pauseWithCompletionHandler(&completion),
      Action::Resume => vm.machine.resumeWithCompletionHandler(&completion),
      Action::Stop => vm.machine.stopWithCompletionHandler(&completion),
    }
  }
  Ok(Pending(receive))
}

pub fn poll(pending: &Pending) -> anyhow::Result<Option<anyhow::Result<()>>> {
  match pending.0.try_recv() {
    Ok(result) => Ok(Some(result)),
    Err(mpsc::TryRecvError::Empty) => Ok(None),
    Err(error) => Err(error).context("VZ completion handler ended without a result"),
  }
}
