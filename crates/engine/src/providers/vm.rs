use anyhow::{bail, Context};
use async_trait::async_trait;
use docker::{Client, Endpoint};
use model::{EngineResources, EngineState, EngineStats, EngineStatus, ReclaimResult, Settings};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::Mutex;

use crate::provider::Provider;
use crate::vm::{
    cli::{self, Cli},
    config::{self, INSTANCE},
};

pub struct Vm {
    #[cfg(test)]
    helper: Option<PathBuf>,
    home: PathBuf,
    lifecycle: Mutex<()>,
    cancellation: tokio::sync::watch::Sender<u64>,
    starting: AtomicBool,
    phase: std::sync::RwLock<&'static str>,
    failure: std::sync::RwLock<Option<String>>,
}

struct Starting<'a>(&'a AtomicBool);

impl Drop for Starting<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl Default for Vm {
    fn default() -> Self {
        Self::at(store::paths::engine_dir().join("lima"))
    }
}

impl Vm {
    pub fn at(home: PathBuf) -> Self {
        Self {
            #[cfg(test)]
            helper: None,
            home,
            lifecycle: Mutex::new(()),
            cancellation: tokio::sync::watch::channel(0).0,
            starting: AtomicBool::new(false),
            phase: std::sync::RwLock::new("Preparing Hopper's Linux VM…"),
            failure: std::sync::RwLock::new(None),
        }
    }

    fn cli(&self) -> anyhow::Result<Cli> {
        #[cfg(test)]
        if let Some(bin) = &self.helper {
            return Ok(Cli {
                bin: bin.clone(),
                home: self.home.clone(),
            });
        }
        Ok(Cli {
            bin: cli::locate().context("Hopper's VM helper is missing. Run scripts/build/lima.sh for a development build, or reinstall Hopper.")?,
            home: self.home.clone(),
        })
    }

    fn socket(&self) -> PathBuf {
        self.home.join(INSTANCE).join("sock/docker.sock")
    }

    fn probe(&self) -> Client {
        let client = Client::new(Endpoint::Unix {
            path: self.socket().to_string_lossy().into(),
        });
        client.set_timeout(Duration::from_secs(3));
        client
    }

    fn status(&self, state: EngineState, message: &str) -> EngineStatus {
        EngineStatus::new(state, "vm", message)
            .managed(true)
            .endpoint(format!("unix://{}", self.socket().display()))
    }

    async fn start_vm(&self, settings: &Settings) -> anyhow::Result<()> {
        if !self.available().await {
            bail!("Hopper's VM helper is unavailable on this machine.");
        }
        let cli = self.cli()?;
        let _process = cli::lifecycle(&self.home).await?;
        let mut settings = settings.clone();
        settings.resources = settings
            .resources
            .bounded_to(crate::vm::resources::capacity());
        let resources = settings.resources;
        let mounts = config::mount_expression(&settings)?;
        let daemon = crate::vm::daemon::document(&settings.daemon)?;
        let desired =
            serde_json::json!({"resources": resources, "mounts": mounts, "daemon": daemon});
        let instance = cli.instance().await?;
        if instance.as_ref().is_some_and(|i| i.status == "Running") {
            *self.phase.write().unwrap() = "Checking the running Docker engine…";
            // One failed ping is not sufficient reason to interrupt workloads.
            if self.wait_ready_for(Duration::from_secs(15)).await.is_ok() {
                let applied: serde_json::Value =
                    store::json::read_or_default(&self.home.join("applied.json"));
                anyhow::ensure!(applied == desired, "The VM is already running. Stop and start it to apply VM resources, file shares, and Docker settings.");
                return Ok(());
            }
            tracing::warn!(
                "Docker did not recover within the grace period; restarting Hopper's VM"
            );
            cli.run(&["stop".into(), INSTANCE.into()], Duration::from_secs(120))
                .await?;
        }
        if instance.is_some() {
            cli.run(
                &[
                    "edit".into(),
                    format!("--cpus={}", resources.cpus),
                    format!("--memory={}", resources.memory_gib),
                    "--mount-none".into(),
                    format!("--set={mounts}"),
                    format!("--set={}", config::provision_expression()?),
                    INSTANCE.into(),
                ],
                Duration::from_secs(30),
            )
            .await?;
        } else {
            *self.phase.write().unwrap() = "Downloading Linux and preparing the VM disk…";
            let profile = self.home.join("hopper.yaml");
            store::json::write_bytes(&profile, config::render_configured(&settings)?.as_bytes())?;
            cli.run(
                &[
                    "create".into(),
                    "--name=hopper".into(),
                    "--mount-none".into(),
                    format!("--set={mounts}"),
                    profile.to_string_lossy().into(),
                ],
                Duration::from_secs(900),
            )
            .await?;
        }
        *self.phase.write().unwrap() = "Starting the VM and setting up Docker…";
        cli.run(
            &["start".into(), "--timeout=10m".into(), INSTANCE.into()],
            Duration::from_secs(660),
        )
        .await?;
        *self.phase.write().unwrap() = "Applying and verifying Docker settings…";
        let script = crate::vm::daemon::apply_script(&settings.daemon)?;
        cli.input(
            &[
                "shell".into(),
                "--workdir=/".into(),
                INSTANCE.into(),
                "sudo".into(),
                "sh".into(),
                "-s".into(),
            ],
            script.as_bytes(),
            Duration::from_secs(90),
        )
        .await?;
        *self.phase.write().unwrap() = "Waiting for Docker to become ready…";
        self.wait_ready_for(Duration::from_secs(60)).await?;
        store::json::write(&self.home.join("applied.json"), &desired)?;
        Ok(())
    }

    async fn wait_ready_for(&self, grace: Duration) -> anyhow::Result<()> {
        let probe = self.probe();
        let deadline = tokio::time::Instant::now() + grace;
        loop {
            if tokio::time::timeout_at(deadline, probe.ping())
                .await
                .is_ok_and(|r| r.is_ok())
            {
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                bail!("The VM started but Docker did not become ready. Retry to restart the VM. See {}", self.home.join("engine.log").display());
            }
            tokio::time::sleep_until(
                (tokio::time::Instant::now() + Duration::from_secs(1)).min(deadline),
            )
            .await;
        }
    }
}

#[async_trait]
impl Provider for Vm {
    fn id(&self) -> &'static str {
        "vm"
    }
    fn label(&self) -> &'static str {
        "Hopper Engine"
    }
    fn managed(&self) -> bool {
        true
    }

    fn starting(&self) -> bool {
        self.starting.load(Ordering::Acquire)
    }

    async fn available(&self) -> bool {
        #[cfg(test)]
        if self.helper.is_some() {
            return true;
        }
        cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") && cli::locate().is_some()
    }

    async fn endpoint(&self) -> Option<Endpoint> {
        Some(Endpoint::Unix {
            path: self.socket().to_string_lossy().into(),
        })
    }

    async fn status(&self) -> EngineStatus {
        if !cfg!(target_os = "macos") || !cfg!(target_arch = "aarch64") {
            return self.status(
                EngineState::Unsupported,
                "Hopper's VM requires an Apple silicon Mac.",
            );
        }
        if self.starting.load(Ordering::Acquire) {
            return self.status(EngineState::Starting, *self.phase.read().unwrap())
                .detail("First startup downloads Linux and installs Docker. Later starts reuse the same disk.");
        }
        let cli = match self.cli() {
            Ok(cli) => cli,
            Err(error) => {
                return self
                    .status(EngineState::NotInstalled, "Hopper's VM helper is missing.")
                    .detail(error.to_string())
            }
        };
        // A healthy socket is sufficient and avoids spawning limactl every
        // status tick. The CLI is needed only to explain a failed probe.
        let probe_error = match self.probe().ping().await {
            Ok(()) => {
                return self.status(
                    EngineState::Connected,
                    "Docker is running in Hopper's Linux VM.",
                )
            }
            Err(error) => error,
        };
        match cli.instance().await {
            Ok(Some(instance)) if instance.status == "Running" => self.status(EngineState::Unreachable, "The VM is running, but Docker isn't responding.")
                    .detail(format!("Retry startup checks Docker before restarting the VM. {}", probe_error.message)),
            Ok(Some(instance)) if instance.status != "Stopped" => self.status(EngineState::Unreachable, "Hopper's Linux VM needs recovery.")
                .detail(format!("VM state: {}. See {}", instance.status, self.home.join("engine.log").display())),
            Ok(_) => self.status(EngineState::Stopped, "Hopper's Linux VM is stopped.")
                .detail(self.failure.read().unwrap().clone().unwrap_or_else(|| "Containers and volumes stay on its disk. Starting restores the engine; container restart policies control what runs.".into())),
            Err(error) => self.status(EngineState::Unreachable, "Hopper couldn't check its Linux VM.").detail(error.to_string()),
        }
    }

    async fn start(&self, resources: EngineResources) -> anyhow::Result<()> {
        self.start_configured(&Settings {
            resources,
            ..Default::default()
        })
        .await
    }

    async fn start_configured(&self, settings: &Settings) -> anyhow::Result<()> {
        let mut cancellation = self.cancellation.subscribe();
        cancellation.borrow_and_update();
        let _lifecycle = self.lifecycle.lock().await;
        anyhow::ensure!(
            !cancellation.has_changed().unwrap_or(true),
            "VM startup was cancelled."
        );
        self.starting.store(true, Ordering::Release);
        let _starting = Starting(&self.starting);
        *self.failure.write().unwrap() = None;
        *self.phase.write().unwrap() = "Preparing Hopper's Linux VM…";
        let result = tokio::select! {
            result = self.start_vm(settings) => result,
            _ = cancellation.changed() => Err(anyhow::anyhow!("VM startup was cancelled.")),
        };
        *self.failure.write().unwrap() = result.as_ref().err().map(|error| format!("{error:#}"));
        result
    }

    async fn stop(&self) -> anyhow::Result<()> {
        self.cancellation
            .send_modify(|generation| *generation = generation.wrapping_add(1));
        let _lifecycle = self.lifecycle.lock().await;
        let _process = cli::lifecycle(&self.home).await?;
        let cli = self.cli()?;
        if cli.instance().await?.is_some_and(|i| i.status != "Stopped") {
            cli.run(&["stop".into(), INSTANCE.into()], Duration::from_secs(120))
                .await?;
        }
        *self.failure.write().unwrap() = None;
        Ok(())
    }

    async fn idle(&self) -> bool {
        let Ok(cli) = self.cli() else {
            return false;
        };
        let script = r#"#!/bin/bash
set -euo pipefail
command -v ss >/dev/null
command -v fuser >/dev/null
# Never interrupt a signed Ubuntu package update, even with no containers.
if fuser -s /var/lib/dpkg/lock /var/lib/dpkg/lock-frontend /var/lib/apt/lists/lock; then
  exit 1
fi
connections=$(ss -Hxn state connected)
if printf '%s\n' "$connections" | awk '$5 == "/run/docker.sock" || $5 == "/var/run/docker.sock" {found=1} END {exit !found}'; then
  exit 1
fi
printf 'idle\n'
"#;
        cli.input(
            &[
                "shell".into(),
                "--workdir=/".into(),
                INSTANCE.into(),
                "sudo".into(),
                "bash".into(),
                "-s".into(),
            ],
            script.as_bytes(),
            Duration::from_secs(5),
        )
        .await
        .is_ok_and(|output| output.trim() == "idle")
    }

    async fn stats(&self) -> Option<EngineStats> {
        let cli = self.cli().ok()?;
        let output = cli
            .output(
                &[
                    "shell".into(),
                    "--workdir=/".into(),
                    INSTANCE.into(),
                    "sh".into(),
                    "-c".into(),
                    crate::vm::stats::SCRIPT.into(),
                ],
                Duration::from_secs(5),
            )
            .await
            .ok()?;
        crate::vm::stats::parse(&output)
    }

    async fn reclaim(&self) -> ReclaimResult {
        let result = async {
            let _lifecycle = self.lifecycle.lock().await;
            let _process = cli::lifecycle(&self.home).await?;
            self.cli()?
                .output(
                    &[
                        "shell".into(),
                        "--workdir=/".into(),
                        INSTANCE.into(),
                        "sudo".into(),
                        "fstrim".into(),
                        "--verbose".into(),
                        "/".into(),
                    ],
                    Duration::from_secs(60),
                )
                .await?;
            Ok::<_, anyhow::Error>(())
        }
        .await;
        match result {
            Ok(()) => ReclaimResult {ok:true, detail:"Trim completed. Unused VM disk blocks can now be returned to the host; no containers, images, or volumes were deleted.".into()},
            Err(error) => ReclaimResult {ok:false, detail:format!("Disk reclaim failed: {error:#}")},
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Arc;

    #[tokio::test]
    async fn stop_cancels_a_boot_blocked_in_a_silent_helper() {
        let directory = tempfile::tempdir().unwrap();
        let bin = directory.path().join("helper");
        std::fs::write(&bin, "#!/bin/sh\nif [ ! -f \"$LIMA_HOME/listed\" ]; then\n touch \"$LIMA_HOME/listed\"\n exec sleep 600\nfi\nprintf '%s\\n' '{\"name\":\"hopper\",\"status\":\"Stopped\"}'\n").unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o700)).unwrap();
        let home = directory.path().join("vm");
        std::fs::create_dir_all(home.join("hopper")).unwrap();
        let mut vm = Vm::at(home.clone());
        vm.helper = Some(bin);
        let vm = Arc::new(vm);
        let starting = Arc::clone(&vm);
        let boot = tokio::spawn(async move { starting.start(EngineResources::default()).await });
        tokio::time::timeout(Duration::from_secs(3), async {
            while !home.join("listed").exists() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        tokio::time::timeout(Duration::from_secs(3), vm.stop())
            .await
            .unwrap()
            .unwrap();
        assert!(boot
            .await
            .unwrap()
            .unwrap_err()
            .to_string()
            .contains("cancelled"));
        assert!(!vm.starting());
        assert!(cli::lifecycle(&home).await.is_ok());
    }
}
