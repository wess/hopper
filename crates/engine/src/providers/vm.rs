use anyhow::{bail, Context};
use async_trait::async_trait;
use docker::{Client, Endpoint};
use model::{EngineResources, EngineState, EngineStatus};
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
    home: PathBuf,
    lifecycle: Mutex<()>,
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
            home,
            lifecycle: Mutex::new(()),
            starting: AtomicBool::new(false),
            phase: std::sync::RwLock::new("Preparing Hopper's Linux VM…"),
            failure: std::sync::RwLock::new(None),
        }
    }

    fn cli(&self) -> anyhow::Result<Cli> {
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

    async fn start_vm(&self, resources: EngineResources) -> anyhow::Result<()> {
        if !self.available().await {
            bail!("Hopper's VM helper is unavailable on this machine.");
        }
        let cli = self.cli()?;
        tokio::fs::create_dir_all(&self.home).await?;
        let instance = cli.instance().await?;
        if instance.as_ref().is_some_and(|i| i.status == "Running") {
            if self.probe().ping().await.is_ok() {
                return Ok(());
            }
            cli.run(&["stop".into(), INSTANCE.into()], Duration::from_secs(120))
                .await?;
        }
        let resources = resources.bounded();
        if instance.is_some() {
            cli.run(
                &[
                    "edit".into(),
                    format!("--cpus={}", resources.cpus),
                    format!("--memory={}", resources.memory_gib),
                    INSTANCE.into(),
                ],
                Duration::from_secs(30),
            )
            .await?;
        } else {
            *self.phase.write().unwrap() = "Downloading Linux and preparing the VM disk…";
            let home =
                std::env::var_os("HOME").context("The user's home folder could not be found")?;
            let home = PathBuf::from(home);
            let profile = self.home.join("hopper.yaml");
            tokio::fs::write(&profile, config::render(resources, &home)?).await?;
            cli.run(
                &[
                    "create".into(),
                    "--name=hopper".into(),
                    format!("--mount-only={}:w", home.display()),
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
        *self.phase.write().unwrap() = "Waiting for Docker to become ready…";
        self.wait_ready().await
    }

    async fn wait_ready(&self) -> anyhow::Result<()> {
        let probe = self.probe();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
        loop {
            if probe.ping().await.is_ok() {
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                bail!("The VM started but Docker did not become ready. Retry to restart the VM. See {}", self.home.join("engine.log").display());
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
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
        match cli.instance().await {
            Ok(Some(instance)) if instance.status == "Running" => match self.probe().ping().await {
                Ok(_) => self.status(EngineState::Connected, "Docker is running in Hopper's Linux VM."),
                Err(error) => self.status(EngineState::Unreachable, "The VM is running, but Docker isn't responding.")
                    .detail(format!("Retry to restart the VM without deleting its disk. {}", error.message)),
            },
            Ok(Some(instance)) if instance.status != "Stopped" => self.status(EngineState::Unreachable, "Hopper's Linux VM needs recovery.")
                .detail(format!("VM state: {}. See {}", instance.status, self.home.join("engine.log").display())),
            Ok(_) => self.status(EngineState::Stopped, "Hopper's Linux VM is stopped.")
                .detail(self.failure.read().unwrap().clone().unwrap_or_else(|| "Containers and volumes stay on its disk. Starting restores the engine; container restart policies control what runs.".into())),
            Err(error) => self.status(EngineState::Unreachable, "Hopper couldn't check its Linux VM.").detail(error.to_string()),
        }
    }

    async fn start(&self, resources: EngineResources) -> anyhow::Result<()> {
        let _lifecycle = self.lifecycle.lock().await;
        self.starting.store(true, Ordering::Release);
        let _starting = Starting(&self.starting);
        *self.failure.write().unwrap() = None;
        *self.phase.write().unwrap() = "Preparing Hopper's Linux VM…";
        let result = self.start_vm(resources).await;
        *self.failure.write().unwrap() = result.as_ref().err().map(|error| format!("{error:#}"));
        result
    }

    async fn stop(&self) -> anyhow::Result<()> {
        let _lifecycle = self.lifecycle.lock().await;
        let cli = self.cli()?;
        if cli.instance().await?.is_some_and(|i| i.status != "Stopped") {
            cli.run(&["stop".into(), INSTANCE.into()], Duration::from_secs(120))
                .await?;
        }
        Ok(())
    }
}
