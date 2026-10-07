use super::Vm;
use anyhow::ensure;
use objc2::rc::Retained;
use objc2_app_kit::NSApplication;
use objc2_virtualization::{VZVirtualMachine, VZVirtualMachineView};
use std::{cell::Cell, rc::Rc, sync::Arc};

/// a display and its runtime ownership stay on the VM queue.
///
/// ```compile_fail
/// fn move_display(display: machine::vz::Display) {
///   std::thread::spawn(move || drop(display));
/// }
/// ```
pub struct Display {
  view: Retained<VZVirtualMachineView>,
  _machine: Retained<VZVirtualMachine>,
  _application: Retained<NSApplication>,
  displaying: Rc<Cell<bool>>,
  _ownership: Option<Arc<dyn Send + Sync>>,
}

pub(super) fn create(vm: &Vm) -> anyhow::Result<Display> {
  ensure!(!vm.displaying.get(), "This VM already has a display");
  let application = NSApplication::sharedApplication(vm._main);
  let view = unsafe { VZVirtualMachineView::new(vm._main) };
  unsafe {
    view.setCapturesSystemKeys(false);
    view.setVirtualMachine(Some(&vm.machine));
  }
  vm.displaying.set(true);
  Ok(Display {
    view,
    _machine: vm.machine.clone(),
    _application: application,
    displaying: vm.displaying.clone(),
    _ownership: Some(super::hold(vm)),
  })
}

impl Display {
  pub fn view(&self) -> &VZVirtualMachineView {
    &self.view
  }
}

impl Drop for Display {
  fn drop(&mut self) {
    unsafe {
      self.view.setVirtualMachine(None);
    }
    self.displaying.set(false);
  }
}
