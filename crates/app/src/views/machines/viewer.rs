#[cfg(target_os = "macos")]
pub fn show(pid: i32) -> anyhow::Result<()> {
    use anyhow::{bail, Context};
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSApplicationActivationOptions, NSRunningApplication};

    let app = NSRunningApplication::runningApplicationWithProcessIdentifier(pid)
        .context("The VM viewer is no longer running")?;
    let thread = MainThreadMarker::new().context("Open the VM viewer from the main thread")?;
    NSApplication::sharedApplication(thread).yieldActivationToApplication(&app);
    if !app.activateFromApplication_options(
        &NSRunningApplication::currentApplication(),
        NSApplicationActivationOptions::ActivateAllWindows,
    ) {
        let mut process = ProcessSerialNumber { high: 0, low: 0 };
        let status = unsafe { GetProcessForPID(pid, &mut process) };
        if status != 0 {
            bail!("Could not find the VM viewer process ({status})");
        }
        // command-line GUI helpers are not always registered as activatable applications.
        let status = unsafe { SetFrontProcessWithOptions(&process, 3) };
        if status != 0 {
            bail!("Could not bring the VM viewer forward ({status})");
        }
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn show(_: i32) -> anyhow::Result<()> {
    anyhow::bail!("VM viewers currently require macOS")
}

#[cfg(target_os = "macos")]
#[repr(C)]
struct ProcessSerialNumber {
    high: u32,
    low: u32,
}

#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn GetProcessForPID(pid: i32, process: *mut ProcessSerialNumber) -> i32;
    fn SetFrontProcessWithOptions(process: *const ProcessSerialNumber, options: u32) -> i32;
}
