use host::{VirtualMachineDisplay, VirtualMachineThread};
use objc2::{rc::Retained, MainThreadOnly};
use objc2_app_kit::{NSBackingStoreType, NSWindow, NSWindowStyleMask};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

pub(super) struct Viewer {
  window: Retained<NSWindow>,
  display: VirtualMachineDisplay,
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
    Ok(Self { window, display })
  }

  pub fn show(&self) {
    self.window.makeKeyAndOrderFront(None);
    self.window.makeFirstResponder(Some(self.display.view()));
  }
}

impl Drop for Viewer {
  fn drop(&mut self) {
    self.window.close();
    self.window.setContentView(None);
  }
}
