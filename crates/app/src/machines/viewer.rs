use host::{VirtualMachineDisplay, VirtualMachineThread};
use objc2::{rc::Retained, MainThreadOnly};
use objc2_app_kit::{NSBackingStoreType, NSWindow, NSWindowStyleMask};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

pub(super) struct Viewer {
  window: Retained<NSWindow>,
  display: Option<VirtualMachineDisplay>,
}

impl Viewer {
  pub fn new(display: VirtualMachineDisplay, title: &str) -> anyhow::Result<Self> {
    let main = VirtualMachineThread::new()
      .ok_or_else(|| anyhow::anyhow!("VM viewers require the main thread"))?;
    let window = unsafe {
      NSWindow::initWithContentRect_styleMask_backing_defer(
        NSWindow::alloc(main),
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1024.0, 768.0)),
        NSWindowStyleMask::Titled
          | NSWindowStyleMask::Closable
          | NSWindowStyleMask::Miniaturizable
          | NSWindowStyleMask::Resizable,
        NSBackingStoreType::Buffered,
        false,
      )
    };
    unsafe {
      window.setReleasedWhenClosed(false);
    }
    window.setTitle(&NSString::from_str(title));
    window.setContentView(Some(display.view()));
    window.setContentMinSize(NSSize::new(640.0, 480.0));
    window.center();
    Ok(Self {
      window,
      display: Some(display),
    })
  }

  pub fn attached(&self) -> bool {
    self.display.is_some()
  }

  pub fn detach(&mut self) {
    self.window.orderOut(None);
    self.window.makeFirstResponder(None);
    self.window.setContentView(None);
    self.display.take();
  }

  pub fn attach(&mut self, display: VirtualMachineDisplay) -> anyhow::Result<()> {
    anyhow::ensure!(self.display.is_none(), "VM viewer is already attached");
    self.window.setContentView(Some(display.view()));
    self.display = Some(display);
    Ok(())
  }

  pub fn show(&self) {
    self.window.makeKeyAndOrderFront(None);
    if let Some(display) = &self.display {
      self.window.makeFirstResponder(Some(display.view()));
    }
  }
}

impl Drop for Viewer {
  fn drop(&mut self) {
    self.window.close();
    self.window.setContentView(None);
  }
}
