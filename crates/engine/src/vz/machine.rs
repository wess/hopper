//! The VM lifecycle: starting, stopping, and reaching the guest over vsock.
//!
//! Virtualization.framework requires every call against a `VZVirtualMachine`
//! to happen on the queue it was created with, and gpui owns the main thread.
//! So the machine lives on its own serial dispatch queue and everything here
//! hops onto it, translating Objective-C completion blocks into futures the
//! rest of the app can await.

use model::EngineState;
use std::sync::{Arc, Mutex};

/// What the supervisor knows about the VM, independent of the framework's own
/// state enum so the rest of the app does not depend on Objective-C types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Stopped,
    Starting,
    Running,
    Stopping,
    Failed,
}

impl Phase {
    pub fn engine_state(&self) -> EngineState {
        match self {
            Phase::Stopped => EngineState::Stopped,
            Phase::Starting | Phase::Stopping => EngineState::Starting,
            Phase::Running => EngineState::Connected,
            Phase::Failed => EngineState::Unreachable,
        }
    }

    /// Whether a start request makes sense right now. Issuing one while the VM
    /// is already coming up would create a second machine against the same
    /// disk image, which corrupts it.
    pub fn can_start(&self) -> bool {
        matches!(self, Phase::Stopped | Phase::Failed)
    }

    pub fn can_stop(&self) -> bool {
        matches!(self, Phase::Running | Phase::Starting)
    }
}

/// The shared phase, written by the queue and read by the UI.
#[derive(Clone, Default)]
pub struct PhaseCell(Arc<Mutex<Option<Phase>>>);

impl PhaseCell {
    pub fn new(initial: Phase) -> Self {
        Self(Arc::new(Mutex::new(Some(initial))))
    }

    pub fn get(&self) -> Phase {
        self.0.lock().unwrap().unwrap_or(Phase::Stopped)
    }

    pub fn set(&self, phase: Phase) {
        *self.0.lock().unwrap() = Some(phase);
    }

    /// Move to `next` only from an expected current phase, reporting whether
    /// the transition happened. This is what stops two concurrent start
    /// requests from both proceeding.
    pub fn transition(&self, from: &[Phase], next: Phase) -> bool {
        let mut slot = self.0.lock().unwrap();
        let current = slot.unwrap_or(Phase::Stopped);
        if from.contains(&current) {
            *slot = Some(next);
            true
        } else {
            false
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use crate::vz::shares::Share;
    use crate::vz::vm::{self, Layout};
    use block2::RcBlock;
    use dispatch2::{DispatchQueue, DispatchQueueAttr, DispatchRetained};
    use model::EngineResources;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::AnyThread;
    use objc2_foundation::NSError;
    use objc2_virtualization::*;
    use std::os::fd::{FromRawFd, RawFd};
    use tokio::sync::oneshot;

    /// Hands a queue-affine Objective-C value to the dispatch queue that owns
    /// it. The framework requires these calls on the machine's own serial
    /// queue, which is exactly where the closure runs — the compiler simply
    /// cannot see that the queue and the value belong together.
    struct OnQueue<T>(T);

    // Safety: every `OnQueue` is unwrapped inside a closure submitted to the
    // one serial queue the contained value is bound to, so it is never touched
    // from two threads.
    unsafe impl<T> Send for OnQueue<T> {}

    /// A connected vsock stream.
    ///
    /// Keeps the framework's connection object alive for as long as the
    /// stream is used; releasing it early closes the connection out from
    /// under whatever is reading.
    pub struct VsockStream {
        inner: tokio::net::UnixStream,
        _connection: OnQueue<Retained<VZVirtioSocketConnection>>,
    }

    impl tokio::io::AsyncRead for VsockStream {
        fn poll_read(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::pin::Pin::new(&mut self.inner).poll_read(cx, buf)
        }
    }

    impl tokio::io::AsyncWrite for VsockStream {
        fn poll_write(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &[u8],
        ) -> std::task::Poll<std::io::Result<usize>> {
            std::pin::Pin::new(&mut self.inner).poll_write(cx, buf)
        }
        fn poll_flush(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::pin::Pin::new(&mut self.inner).poll_flush(cx)
        }
        fn poll_shutdown(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::pin::Pin::new(&mut self.inner).poll_shutdown(cx)
        }
    }

    /// A running (or startable) guest.
    pub struct Machine {
        vm: Retained<VZVirtualMachine>,
        queue: DispatchRetained<DispatchQueue>,
        phase: PhaseCell,
    }

    // The machine is only ever touched through its dispatch queue, which is
    // what makes it safe to hold across threads.
    unsafe impl Send for Machine {}
    unsafe impl Sync for Machine {}

    impl Machine {
        /// Build the VM. The configuration is validated first so a bad setting
        /// surfaces as a message rather than an abort inside the framework.
        pub fn create(
            layout: &Layout,
            resources: &EngineResources,
            shares: &[Share],
            extra_cmdline: &[String],
        ) -> anyhow::Result<Self> {
            if !vm::supported() {
                anyhow::bail!(
                    "This Mac cannot run Hopper's engine: Virtualization is unavailable. \
                     Point Hopper at an existing Docker engine instead."
                );
            }
            let missing = layout.missing(|p| p.exists());
            if !missing.is_empty() {
                anyhow::bail!(
                    "Hopper's engine is missing its guest image ({}). Reinstall Hopper.",
                    missing.join(", ")
                );
            }
            vm::ensure_disk(&layout.disk, resources.disk_gib)?;

            let queue = DispatchQueue::new("io.wess.hopper.vm", DispatchQueueAttr::SERIAL);
            let config =
                unsafe { vm::build_configuration(layout, resources, shares, extra_cmdline) };

            unsafe {
                config.validateWithError().map_err(|e| {
                    anyhow::anyhow!("Hopper's engine configuration was rejected: {e}")
                })?;
            }

            let vm = unsafe {
                VZVirtualMachine::initWithConfiguration_queue(
                    VZVirtualMachine::alloc(),
                    &config,
                    &queue,
                )
            };

            Ok(Self {
                vm,
                queue,
                phase: PhaseCell::new(Phase::Stopped),
            })
        }

        pub fn phase(&self) -> Phase {
            self.phase.get()
        }

        pub fn phase_cell(&self) -> PhaseCell {
            self.phase.clone()
        }

        /// Boot the guest.
        pub async fn start(&self) -> anyhow::Result<()> {
            if !self.phase.transition(&[Phase::Stopped, Phase::Failed], Phase::Starting) {
                // Already coming up or running; a second machine against the
                // same disk image would corrupt it.
                return Ok(());
            }

            let (tx, rx) = oneshot::channel::<Result<(), String>>();
            let tx = Mutex::new(Some(tx));
            let vm = self.vm.clone();
            let handler = RcBlock::new(move |error: *mut NSError| {
                let result = if error.is_null() {
                    Ok(())
                } else {
                    Err(unsafe { (*error).localizedDescription() }.to_string())
                };
                if let Some(tx) = tx.lock().unwrap().take() {
                    let _ = tx.send(result);
                }
            });

            let queue = self.queue.clone();
            let payload = OnQueue((vm, handler));
            queue.exec_async(move || {
                let OnQueue((vm, block)) = &payload;
                unsafe { vm.startWithCompletionHandler(block) };
            });

            match rx.await {
                Ok(Ok(())) => {
                    self.phase.set(Phase::Running);
                    Ok(())
                }
                Ok(Err(message)) => {
                    self.phase.set(Phase::Failed);
                    anyhow::bail!("Hopper's engine did not start: {message}")
                }
                Err(_) => {
                    self.phase.set(Phase::Failed);
                    anyhow::bail!("Hopper's engine did not report whether it started.")
                }
            }
        }

        /// Ask the guest to shut down cleanly, so `dockerd` flushes rather than
        /// losing whatever was mid-write.
        pub async fn stop(&self) -> anyhow::Result<()> {
            if !self.phase.transition(&[Phase::Running, Phase::Starting], Phase::Stopping) {
                return Ok(());
            }

            let (tx, rx) = oneshot::channel::<Result<(), String>>();
            let tx = Mutex::new(Some(tx));
            let vm = self.vm.clone();
            let handler = RcBlock::new(move |error: *mut NSError| {
                let result = if error.is_null() {
                    Ok(())
                } else {
                    Err(unsafe { (*error).localizedDescription() }.to_string())
                };
                if let Some(tx) = tx.lock().unwrap().take() {
                    let _ = tx.send(result);
                }
            });

            let queue = self.queue.clone();
            let payload = OnQueue((vm, handler));
            queue.exec_async(move || {
                let OnQueue((vm, block)) = &payload;
                unsafe {
                    if vm.canRequestStop() {
                        // A guest-initiated shutdown gives dockerd a chance to
                        // flush; stopWithCompletionHandler is the hard kill.
                        let _ = vm.requestStopWithError();
                    }
                    vm.stopWithCompletionHandler(block);
                }
            });

            let _ = rx.await;
            self.phase.set(Phase::Stopped);
            Ok(())
        }

        /// Open a connection to a vsock port inside the guest.
        ///
        /// The framework's file descriptor is only valid while the
        /// `VZVirtioSocketConnection` is alive — dropping it tears the
        /// connection down even if the descriptor was duplicated. So the
        /// object rides along with the stream.
        pub async fn connect(&self, port: u32) -> anyhow::Result<VsockStream> {
            let (tx, rx) = oneshot::channel::<Result<(RawFd, OnQueue<Retained<VZVirtioSocketConnection>>), String>>();
            let tx = Mutex::new(Some(tx));
            let vm = self.vm.clone();

            let handler = RcBlock::new(
                move |connection: *mut VZVirtioSocketConnection, error: *mut NSError| {
                    let result = if !error.is_null() {
                        Err(unsafe { (*error).localizedDescription() }.to_string())
                    } else if connection.is_null() {
                        Err("the guest refused the connection".to_string())
                    } else {
                        // Retain the connection: the descriptor dies with it.
                        let retained = unsafe { Retained::retain(connection) };
                        match retained {
                            Some(conn) => {
                                let fd = unsafe { conn.fileDescriptor() };
                                let dup = unsafe { libc::dup(fd) };
                                if dup < 0 {
                                    Err("could not duplicate the guest socket".to_string())
                                } else {
                                    Ok((dup, OnQueue(conn)))
                                }
                            }
                            None => Err("the guest connection could not be retained".to_string()),
                        }
                    };
                    if let Some(tx) = tx.lock().unwrap().take() {
                        let _ = tx.send(result);
                    }
                },
            );

            let queue = self.queue.clone();
            let payload = OnQueue((vm, handler));
            queue.exec_async(move || {
                let OnQueue((vm, block)) = &payload;
                unsafe {
                    let devices = vm.socketDevices();
                    let Some(device) = devices.firstObject() else {
                        return;
                    };
                    let device: &VZVirtioSocketDevice =
                        &*(Retained::as_ptr(&device) as *const VZVirtioSocketDevice);
                    device.connectToPort_completionHandler(port, block);
                }
            });

            let (fd, connection) = match rx.await {
                Ok(Ok(pair)) => pair,
                Ok(Err(message)) => {
                    anyhow::bail!("Could not reach the guest on vsock port {port}: {message}")
                }
                Err(_) => anyhow::bail!("The guest did not answer on vsock port {port}."),
            };

            let std_stream = unsafe { std::os::unix::net::UnixStream::from_raw_fd(fd) };
            std_stream.set_nonblocking(true)?;
            Ok(VsockStream {
                inner: tokio::net::UnixStream::from_std(std_stream)?,
                _connection: connection,
            })
        }
    }

    // Silence the unused import when the delegate work lands.
    #[allow(dead_code)]
    fn _keep(_: Option<Retained<AnyObject>>) {}
}

#[cfg(target_os = "macos")]
pub use platform::{Machine, VsockStream};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phases_map_onto_engine_states_the_ui_understands() {
        assert_eq!(Phase::Running.engine_state(), EngineState::Connected);
        assert_eq!(Phase::Stopped.engine_state(), EngineState::Stopped);
        // Starting and stopping both read as "in flight" so the UI shows
        // progress rather than flapping between connected and down.
        assert_eq!(Phase::Starting.engine_state(), EngineState::Starting);
        assert_eq!(Phase::Stopping.engine_state(), EngineState::Starting);
        assert_eq!(Phase::Failed.engine_state(), EngineState::Unreachable);
    }

    #[test]
    fn a_start_is_only_allowed_from_a_settled_down_state() {
        assert!(Phase::Stopped.can_start());
        assert!(Phase::Failed.can_start(), "a failed engine can be retried");
        assert!(!Phase::Running.can_start());
        assert!(
            !Phase::Starting.can_start(),
            "a second machine against the same disk would corrupt it"
        );
    }

    #[test]
    fn a_stop_is_allowed_while_still_coming_up() {
        assert!(Phase::Running.can_stop());
        assert!(
            Phase::Starting.can_stop(),
            "a user must be able to cancel a hanging start"
        );
        assert!(!Phase::Stopped.can_stop());
    }

    #[test]
    fn the_phase_cell_starts_where_it_was_told_to() {
        assert_eq!(PhaseCell::new(Phase::Running).get(), Phase::Running);
        assert_eq!(PhaseCell::default().get(), Phase::Stopped);
    }

    #[test]
    fn a_transition_from_an_unexpected_phase_is_refused() {
        let cell = PhaseCell::new(Phase::Running);
        assert!(
            !cell.transition(&[Phase::Stopped], Phase::Starting),
            "starting an already-running engine must not proceed"
        );
        assert_eq!(cell.get(), Phase::Running);
    }

    #[test]
    fn only_the_first_of_two_concurrent_starts_proceeds() {
        let cell = PhaseCell::new(Phase::Stopped);
        let first = cell.transition(&[Phase::Stopped, Phase::Failed], Phase::Starting);
        let second = cell.transition(&[Phase::Stopped, Phase::Failed], Phase::Starting);
        assert!(first);
        assert!(!second, "a double-click on Start must not boot twice");
    }

    #[test]
    fn the_phase_cell_is_shared_between_clones() {
        let cell = PhaseCell::new(Phase::Stopped);
        let clone = cell.clone();
        clone.set(Phase::Running);
        assert_eq!(cell.get(), Phase::Running);
    }
}
