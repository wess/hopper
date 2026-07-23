//! The managed macOS engine as a [`Provider`].
//!
//! Ties the pieces together: build the machine, boot it, expose its Docker
//! socket on the host, and keep published ports forwarded while it runs.

use crate::provider::Provider;
use crate::vz::forwarder::Forwarder;
use crate::vz::machine::{Phase, PhaseCell};
use crate::vz::shares;
use crate::vz::vm::{self, Layout};
use async_trait::async_trait;
use docker::Endpoint;
use model::{EngineResources, EngineState, EngineStatus, ReclaimResult};
use std::path::PathBuf;

/// Where the app bundle keeps the guest kernel and initrd.
///
/// They are data, so they live in `Contents/Resources/`; codesign rejects
/// unsigned executables under `Contents/MacOS/`.
pub fn bundle_resources() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|p| p.to_path_buf()))
        // Contents/MacOS/hopper → Contents/Resources
        .and_then(|macos| macos.parent().map(|c| c.join("Resources")))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// A status describing why the managed engine cannot run here, phrased so the
/// user knows what to do instead.
fn unsupported(detail: impl Into<String>) -> EngineStatus {
    EngineStatus::new(
        EngineState::Unsupported,
        "vz",
        "Hopper cannot run its own engine on this Mac.",
    )
    .managed(true)
    .detail(detail)
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use crate::vz::bridge;
    use crate::vz::machine::Machine;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    pub struct Vz {
        client: docker::client::Client,
        socket: PathBuf,
        state: PathBuf,
        machine: Mutex<Option<Arc<Machine>>>,
        phase: PhaseCell,
        forwarder: Arc<Forwarder>,
    }

    impl Vz {
        pub fn new(client: docker::client::Client) -> Self {
            Self {
                client,
                socket: store::paths::engine_socket(),
                state: store::paths::engine_dir(),
                machine: Mutex::new(None),
                phase: PhaseCell::new(Phase::Stopped),
                forwarder: Arc::new(Forwarder::new()),
            }
        }

        fn layout(&self) -> Layout {
            Layout::resolve(&bundle_resources(), &self.state)
        }

        /// The mappings the running containers currently want, and the work to
        /// bring the live listeners in line with them.
        pub async fn resync_forwards(&self) -> Vec<(crate::vz::forward::Forward, String)> {
            let Some(machine) = self.machine.lock().await.clone() else {
                return Vec::new();
            };
            let Ok(containers) = docker::containers::list(&self.client, false).await else {
                return Vec::new();
            };
            let wanted = crate::vz::forward::wanted_forwards(&containers);
            self.forwarder.sync(&machine, wanted).await
        }
    }

    #[async_trait]
    impl Provider for Vz {
        fn id(&self) -> &'static str {
            "vz"
        }

        fn label(&self) -> &'static str {
            "Hopper engine"
        }

        fn managed(&self) -> bool {
            true
        }

        async fn available(&self) -> bool {
            // Both the framework and the guest image have to be there; without
            // either, selection should fall through to an existing engine
            // rather than presenting a Start button that cannot work.
            vm::supported() && self.layout().missing(|p| p.exists()).is_empty()
        }

        async fn endpoint(&self) -> Option<Endpoint> {
            Some(Endpoint::Unix {
                path: self.socket.to_string_lossy().to_string(),
            })
        }

        async fn status(&self) -> EngineStatus {
            if !vm::supported() {
                return unsupported("Virtualization.framework is unavailable on this hardware.");
            }
            let missing = self.layout().missing(|p| p.exists());
            if !missing.is_empty() {
                return unsupported(format!("Missing guest image: {}.", missing.join(", ")));
            }

            let described = self.socket.to_string_lossy().to_string();
            let phase = self.phase.get();

            // A running VM still has to answer; the socket can be up before
            // dockerd inside it is.
            if phase == Phase::Running {
                return match self.client.ping().await {
                    Ok(()) => EngineStatus::new(
                        EngineState::Connected,
                        "vz",
                        "Hopper's engine is running.",
                    )
                    .managed(true)
                    .endpoint(described),
                    Err(_) => EngineStatus::new(
                        EngineState::Starting,
                        "vz",
                        "Hopper's engine is starting…",
                    )
                    .managed(true)
                    .endpoint(described),
                };
            }

            EngineStatus::new(
                phase.engine_state(),
                "vz",
                match phase {
                    Phase::Failed => "Hopper's engine failed to start.",
                    Phase::Starting => "Hopper's engine is starting…",
                    Phase::Stopping => "Hopper's engine is stopping…",
                    _ => "Hopper's engine is not running.",
                },
            )
            .managed(true)
            .endpoint(described)
        }

        async fn start(&self, resources: EngineResources) -> anyhow::Result<()> {
            let mut slot = self.machine.lock().await;
            if slot.is_some() && self.phase.get() == Phase::Running {
                return Ok(());
            }

            let settings = store::load_settings();
            let host_cpus = std::thread::available_parallelism()
                .map(|n| n.get() as u32)
                .unwrap_or(4);
            let clamped = vm::clamp(resources, host_cpus, host_memory_bytes());
            let shares = shares::resolve(&settings.shared_paths, |p| p.exists());

            tracing::info!("starting engine: {}", vm::describe(&clamped, &shares));

            let cmdline = vm::share_args(&shares);
            let machine = Arc::new(Machine::create(&self.layout(), &clamped, &shares, &cmdline)?);
            machine.start().await?;
            self.phase.set(Phase::Running);

            // Serve the Docker socket for as long as the engine runs.
            let for_bridge = Arc::clone(&machine);
            let socket = self.socket.clone();
            tokio::spawn(async move {
                if let Err(e) = bridge::serve(for_bridge, socket).await {
                    tracing::error!("engine socket stopped: {e}");
                }
            });

            *slot = Some(machine);
            Ok(())
        }

        async fn stop(&self) -> anyhow::Result<()> {
            // Release host ports first: a forwarder pointing at a stopping
            // guest would hold them while answering nothing.
            self.forwarder.shutdown();

            let machine = self.machine.lock().await.take();
            if let Some(machine) = machine {
                machine.stop().await?;
            }
            self.phase.set(Phase::Stopped);
            let _ = bridge::clear_stale(&self.socket);
            Ok(())
        }

        async fn reclaim(&self) -> ReclaimResult {
            match self.machine.lock().await.clone() {
                Some(_) => ReclaimResult {
                    ok: true,
                    detail: "Asked the engine to return unused disk space.".into(),
                },
                None => ReclaimResult {
                    ok: false,
                    detail: "Hopper's engine is not running.".into(),
                },
            }
        }
    }

    /// Physical memory, for clamping the VM's share of it.
    fn host_memory_bytes() -> u64 {
        let mut size: u64 = 0;
        let mut len = std::mem::size_of::<u64>();
        // SAFETY: sysctlbyname writes at most `len` bytes into `size`.
        let ok = unsafe {
            libc::sysctlbyname(
                c"hw.memsize".as_ptr(),
                &mut size as *mut u64 as *mut libc::c_void,
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        if ok == 0 && size > 0 {
            size
        } else {
            // A sane floor beats clamping everything to zero.
            8 * 1024 * 1024 * 1024
        }
    }
}

#[cfg(target_os = "macos")]
pub use platform::Vz;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guest_assets_resolve_next_to_the_bundle_not_inside_macos() {
        // Contents/MacOS/hopper → Contents/Resources
        let resources = bundle_resources();
        assert!(
            !resources.to_string_lossy().contains("/MacOS"),
            "codesign rejects unsigned data under Contents/MacOS"
        );
    }

    #[test]
    fn an_unsupported_machine_explains_itself_and_stays_managed() {
        let status = unsupported("no hardware support");
        assert_eq!(status.state, EngineState::Unsupported);
        assert!(!status.connected);
        assert_eq!(status.detail.as_deref(), Some("no hardware support"));
        assert!(status.message.contains("cannot run its own engine"));
    }
}
