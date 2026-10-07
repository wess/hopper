mod audio;
mod auxiliary;
mod config;
mod console;
mod display;
pub mod install;
pub mod kernel;
pub mod mac;
pub mod network;
pub mod queue;
pub mod restore;
pub mod sharing;

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
  pub seed: Option<PathBuf>,
  pub network: Option<network::Mode>,
  pub console: Option<std::fs::File>,
  pub shares: Vec<sharing::Directory>,
  pub speakers: bool,
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
  audio_devices: usize,
  installing: Rc<Cell<bool>>,
  mac_ready: Rc<Cell<bool>>,
  ownership: Option<std::sync::Arc<dyn Send + Sync>>,
  shares: std::sync::Arc<Vec<sharing::Directory>>,
  displaying: Rc<Cell<bool>>,
  installer: bool,
  started: Cell<bool>,
  stop_requested: Rc<Cell<bool>>,
  generation: Cell<u64>,
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
  Ok(configured(main, &config, false, &boot.shares))
}

pub fn create_mac(main: MainThreadMarker, boot: &mac::Mac) -> anyhow::Result<Vm> {
  let config = mac::config(boot)?;
  Ok(configured(main, &config, true, &boot.shares))
}

// the caller validates a durable successful-installation receipt before recovering hardware.
pub fn recover_mac(main: MainThreadMarker, boot: &mac::Mac) -> anyhow::Result<Vm> {
  let vm = create_mac(main, boot)?;
  vm.mac_ready.set(true);
  Ok(vm)
}

fn configured(
  main: MainThreadMarker,
  config: &objc2_virtualization::VZVirtualMachineConfiguration,
  mac: bool,
  shares: &[sharing::Directory],
) -> Vm {
  let machine =
    unsafe { VZVirtualMachine::initWithConfiguration(VZVirtualMachine::alloc(), config) };
  Vm {
    machine,
    _main: main,
    mac,
    audio_devices: unsafe { config.audioDevices().len() },
    installing: Rc::new(Cell::new(false)),
    mac_ready: Rc::new(Cell::new(!mac)),
    ownership: None,
    shares: std::sync::Arc::new(shares.to_vec()),
    displaying: Rc::new(Cell::new(false)),
    installer: false,
    started: Cell::new(false),
    stop_requested: Rc::new(Cell::new(false)),
    generation: Cell::new(0),
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

pub fn restrict_installer_restart(vm: &mut Vm) -> anyhow::Result<()> {
  ensure!(
    state(vm) == VZVirtualMachineState::Stopped && !vm.installing.get() && !vm.started.get(),
    "Restrict installer restart before hardware startup"
  );
  vm.installer = true;
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
  ensure!(
    !matches!(action, Action::Start) || vm.mac_ready.get(),
    "Install macOS before starting its hardware"
  );
  if matches!(action, Action::Start) {
    for directory in &*vm.shares {
      directory.validate()?;
    }
  }
  let (send, receive) = mpsc::sync_channel(1);
  let held = Rc::new(RefCell::new(Some((
    vm.machine.clone(),
    check,
    Some(hold(vm)),
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
  if matches!(action, Action::Stop) {
    vm.stop_requested.set(true);
  }
  unsafe {
    let permitted = match action {
      Action::Start => vm.machine.canStart(),
      Action::Pause => vm.machine.canPause(),
      Action::Resume => vm.machine.canResume(),
      Action::Stop => vm.machine.canStop(),
    };
    ensure!(permitted, "VZ state does not permit this transition");
    if matches!(action, Action::Start) {
      ensure!(
        !vm.installer || !vm.started.get(),
        "Installer has already started; preserve this VM for installation recovery or system boot"
      );
      vm.started.set(true);
    }
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

fn hold(vm: &Vm) -> std::sync::Arc<dyn Send + Sync> {
  std::sync::Arc::new((vm.shares.clone(), vm.ownership.clone()))
}
