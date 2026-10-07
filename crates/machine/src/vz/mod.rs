mod config;
mod console;
mod kernel;

use anyhow::{ensure, Context};
use block2::RcBlock;
pub use config::{create_variables, identity};
use objc2::{rc::Retained, AllocAnyThread, MainThreadMarker};
use objc2_foundation::NSError;
use objc2_virtualization::{VZVirtualMachine, VZVirtualMachineState};
use std::{path::PathBuf, sync::mpsc};

pub struct Linux {
  pub cpus: usize,
  pub memory: u64,
  pub width: usize,
  pub height: usize,
  pub identity: Vec<u8>,
  pub boot: Boot,
  pub disk: PathBuf,
  pub installer: Option<PathBuf>,
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
  let machine =
    unsafe { VZVirtualMachine::initWithConfiguration(VZVirtualMachine::alloc(), &config) };
  Ok(Vm {
    machine,
    _main: main,
  })
}

pub fn state(vm: &Vm) -> VZVirtualMachineState {
  unsafe { vm.machine.state() }
}

pub fn transition(vm: &Vm, action: Action) -> anyhow::Result<Pending> {
  let (send, receive) = mpsc::sync_channel(1);
  let completion = RcBlock::new(move |error: *mut NSError| {
    let result = if error.is_null() {
      Ok(())
    } else {
      let message: String = unsafe { &*error }.to_string().chars().take(512).collect();
      Err(anyhow::anyhow!("VZ transition failed: {message}"))
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
