//! User VMs have their own Lima home, independent of the Docker engine.

mod cli;
pub mod config;
mod input;
mod macos;
pub mod media;
pub mod native;
mod qmp;
mod snapshots;
pub mod windows;

use anyhow::{bail, Context};
use model::{CreateMachine, Machine, MachineProfile, MachineStatus};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Actor {
    Person,
    Agent,
}

#[derive(Clone)]
pub struct Machines {
    pub root: PathBuf,
}

impl Default for Machines {
    fn default() -> Self {
        Self {
            root: store::paths::root().join("machines"),
        }
    }
}

#[derive(Deserialize)]
struct Instance {
    name: String,
    status: String,
}

fn validate_id(id: &str) -> anyhow::Result<()> {
    if id.len() != 36
        || !id.bytes().enumerate().all(|(i, c)| {
            if [8, 13, 18, 23].contains(&i) {
                c == b'-'
            } else {
                c.is_ascii_hexdigit()
            }
        })
    {
        bail!("Invalid VM id");
    }
    Ok(())
}

impl Machines {
    pub fn profiles(&self) -> Vec<MachineProfile> {
        config::profiles()
    }

    fn cli(&self) -> anyhow::Result<cli::Cli> {
        Ok(cli::Cli {
            bin: crate::vm::cli::locate().context("Hopper's VM helper is unavailable")?,
            home: self.root.join("lima"),
        })
    }

    fn record(&self, id: &str) -> anyhow::Result<PathBuf> {
        validate_id(id)?;
        Ok(self.root.join("records").join(format!("{id}.json")))
    }

    pub fn machine(&self, id: &str, actor: Actor) -> anyhow::Result<Machine> {
        let machine: Machine =
            serde_json::from_slice(&std::fs::read(self.record(id)?).context("VM not found")?)?;
        if machine.id != id {
            bail!("VM record does not match its id");
        }
        if actor == Actor::Agent && !machine.agent_access {
            bail!("Agent access is off for this VM. Enable it in Hopper first.");
        }
        Ok(machine)
    }

    fn lock(&self, id: &str) -> anyhow::Result<store::lock::Lease> {
        self.guard(id, "")
    }

    fn guard(&self, id: &str, suffix: &str) -> anyhow::Result<store::lock::Lease> {
        validate_id(id)?;
        let dir = self.root.join("locks");
        std::fs::create_dir_all(&dir)?;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(dir.join(format!("{id}{suffix}")))?;
        store::lock::exclusive(file).context("Another operation is in progress for this VM")
    }

    pub fn set_agent_access(&self, id: &str, enabled: bool) -> anyhow::Result<()> {
        validate_id(id)?;
        let dir = self.root.join("locks");
        std::fs::create_dir_all(&dir)?;
        let policy_lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(dir.join(format!("{id}.access")))?;
        let _policy_lock = store::lock::exclusive(policy_lock)
            .context("Another agent access change is in progress")?;
        let mut machine = self.machine(id, Actor::Person)?;
        machine.agent_access = enabled;
        store::json::write(&self.record(id)?, &machine)?;
        Ok(())
    }

    pub async fn list(&self, actor: Actor) -> anyhow::Result<Vec<MachineStatus>> {
        let dir = self.root.join("records");
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let output = self
            .cli()?
            .output(&["list".into(), "--json".into()], Duration::from_secs(10))
            .await?;
        let instances: Vec<Instance> = output
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(serde_json::from_str)
            .collect::<Result<_, _>>()?;
        let mut result = Vec::new();
        for file in std::fs::read_dir(dir)? {
            let path = file?.path();
            if path.extension().is_none_or(|ext| ext != "json") {
                continue;
            }
            let machine: Machine = serde_json::from_slice(&std::fs::read(&path)?)?;
            validate_id(&machine.id)?;
            if actor == Actor::Agent && !machine.agent_access {
                continue;
            }
            let state = instances
                .iter()
                .find(|i| i.name == machine.id)
                .map(|i| i.status.clone())
                .unwrap_or_else(|| "Not created".into());
            let busy = match self.lock(&machine.id) {
                Ok(_lock) => false,
                Err(error)
                    if error
                        .downcast_ref::<std::io::Error>()
                        .is_some_and(|e| e.kind() == std::io::ErrorKind::WouldBlock) =>
                {
                    true
                }
                Err(error) => return Err(error),
            };
            result.push(MachineStatus {
                state,
                progress: busy
                    .then(|| {
                        std::fs::read_to_string(self.root.join("progress").join(&machine.id)).ok()
                    })
                    .flatten(),
                busy,
                machine,
            });
        }
        result.sort_by(|a, b| {
            a.machine
                .name
                .to_lowercase()
                .cmp(&b.machine.name.to_lowercase())
        });
        Ok(result)
    }

    pub async fn create(&self, request: CreateMachine) -> anyhow::Result<Machine> {
        if !cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            bail!("Desktop VMs currently require an Apple silicon Mac");
        }
        if request.name.trim().is_empty() || request.name.len() > 100 {
            bail!("Choose a VM name of 1–100 characters");
        }
        let profile = self
            .profiles()
            .into_iter()
            .find(|p| p.id == request.profile)
            .context("Unknown VM profile")?;
        let machine = Machine {
            id: model::new_uuid(),
            name: request.name.trim().into(),
            guest: profile.guest,
            profile: profile.id,
            resources: request.resources,
            installer: request.installer,
            agent_access: request.agent_access,
        };
        let yaml = config::render(&machine)?;
        let _lock = self.lock(&machine.id)?;
        let cli = self.cli()?;
        std::fs::create_dir_all(&cli.home)?;
        let config_dir = self.root.join("configs");
        std::fs::create_dir_all(&config_dir)?;
        let path = config_dir.join(format!("{}.yaml", machine.id));
        std::fs::write(&path, yaml)?;
        store::json::write(&self.record(&machine.id)?, &machine)?;
        Ok(machine)
    }

    pub async fn start(&self, id: &str, actor: Actor) -> anyhow::Result<()> {
        let _lock = self.lock(id)?;
        let mut machine = self.machine(id, actor)?;
        if machine.guest == model::GuestOs::Windows && !cli::windows_ready() {
            bail!(
                "The Windows runtime is missing. Hopper needs QEMU and swtpm bundled with the app."
            );
        }
        let progress = self.root.join("progress").join(id);
        std::fs::create_dir_all(progress.parent().unwrap())?;
        struct Progress(PathBuf);
        impl Drop for Progress {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }
        let _progress = Progress(progress.clone());
        let cli = self.cli()?;
        if !cli.home.join(id).exists()
            && machine.guest == model::GuestOs::Windows
            && machine.installer.is_none()
        {
            machine.installer = Some(
                media::prepare(&self.root, &progress)
                    .await?
                    .to_string_lossy()
                    .into_owned(),
            );
            std::fs::write(
                self.root.join("configs").join(format!("{id}.yaml")),
                config::render(&machine)?,
            )?;
            store::json::write(&self.record(id)?, &machine)?;
        }
        std::fs::write(&progress, "Starting VM and preparing its desktop…")?;
        if !cli.home.join(id).exists() {
            let path = self.root.join("configs").join(format!("{id}.yaml"));
            cli.run(
                id,
                &[
                    "create".into(),
                    format!("--name={id}"),
                    "--mount-none".into(),
                    path.to_string_lossy().into_owned(),
                ],
                Duration::from_secs(31 * 60),
            )
            .await?;
        }
        let windows = machine.guest == model::GuestOs::Windows;
        let timeout = if windows {
            "--timeout=15s"
        } else {
            "--timeout=30m"
        };
        let result = cli
            .run(
                id,
                &["start".into(), id.into(), timeout.into()],
                Duration::from_secs(31 * 60),
            )
            .await;
        if result.is_err() && windows {
            // Windows setup can wait for user input before SSH becomes available.
            // A live display is enough to return control to the viewer and agents.
            if qmp::execute(
                &cli.home.join(id).join("qmp.sock"),
                serde_json::json!({"execute":"query-status"}),
            )
            .await
            .is_ok()
            {
                return Ok(());
            }
        }
        result
    }

    pub async fn viewer_pid(&self, id: &str) -> anyhow::Result<i32> {
        let _lock = self.lock(id)?;
        let machine = self.machine(id, Actor::Person)?;
        let output = self
            .cli()?
            .output(&["list".into(), "--json".into()], Duration::from_secs(10))
            .await?;
        let running = output
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(serde_json::from_str::<Instance>)
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .any(|instance| instance.name == id && instance.status == "Running");
        if !running {
            bail!("Start this VM before opening its viewer");
        }
        let file = if machine.guest == model::GuestOs::Windows {
            "qemu.pid"
        } else {
            "vz.pid"
        };
        let pid: i32 = std::fs::read_to_string(self.root.join("lima").join(id).join(file))?
            .trim()
            .parse()?;
        if pid <= 0 {
            bail!("Invalid VM viewer process");
        }
        Ok(pid)
    }

    pub async fn stop(&self, id: &str, actor: Actor) -> anyhow::Result<()> {
        let _lock = self.lock(id)?;
        self.machine(id, actor)?;
        self.cli()?
            .run(id, &["stop".into(), id.into()], Duration::from_secs(120))
            .await
    }

    pub async fn exec(&self, id: &str, actor: Actor, args: &[String]) -> anyhow::Result<String> {
        let _lock = self.lock(id)?;
        self.machine(id, actor)?;
        if args.is_empty() {
            bail!("Provide a guest command and its arguments");
        }
        let mut command = vec!["shell".into(), id.into(), "--".into()];
        command.extend_from_slice(args);
        self.cli()?.output(&command, Duration::from_secs(120)).await
    }

    pub async fn screenshot(&self, id: &str, actor: Actor, path: &Path) -> anyhow::Result<()> {
        let machine = self.machine(id, actor)?;
        let _lock = self.guard(
            id,
            if machine.guest == model::GuestOs::Windows {
                ".display"
            } else {
                ""
            },
        )?;
        let machine = self.machine(id, actor)?;
        let cli = self.cli()?;
        if machine.guest == model::GuestOs::Windows {
            return qmp::screenshot(&cli.home.join(id).join("qmp.sock"), path).await;
        }
        if machine.guest == model::GuestOs::Linux {
            let guest = format!("/tmp/hopper-screen-{}.png", model::new_uuid());
            cli.output(&["shell".into(),id.into(),"--".into(),"sh".into(),"-c".into(),"export DISPLAY=:0 XAUTHORITY=\"$HOME/.Xauthority\"; exec scrot --overwrite \"$1\"".into(),"hopper-screen".into(),guest.clone()],Duration::from_secs(15)).await?;
            let result = cli
                .run(
                    id,
                    &[
                        "copy".into(),
                        "--".into(),
                        format!("{id}:{guest}"),
                        path.to_string_lossy().into_owned(),
                    ],
                    Duration::from_secs(30),
                )
                .await;
            let _ = cli
                .output(
                    &[
                        "shell".into(),
                        id.into(),
                        "--".into(),
                        "rm".into(),
                        "--".into(),
                        guest,
                    ],
                    Duration::from_secs(5),
                )
                .await;
            return result;
        }
        cli.run(
            id,
            &[
                "screenshot".into(),
                id.into(),
                "--output".into(),
                path.to_string_lossy().into_owned(),
            ],
            Duration::from_secs(15),
        )
        .await
    }

    pub async fn copy(
        &self,
        id: &str,
        actor: Actor,
        host: &Path,
        guest: &str,
        upload: bool,
    ) -> anyhow::Result<()> {
        let _lock = self.lock(id)?;
        self.machine(id, actor)?;
        if !host.is_absolute() || guest.is_empty() || guest.contains('\0') {
            bail!("Provide an absolute host path and a guest path");
        }
        let local = host.to_string_lossy().into_owned();
        let remote = format!("{id}:{guest}");
        let paths = if upload {
            [local, remote]
        } else {
            [remote, local]
        };
        self.cli()?
            .run(
                id,
                &[
                    "copy".into(),
                    "--".into(),
                    paths[0].clone(),
                    paths[1].clone(),
                ],
                Duration::from_secs(120),
            )
            .await
    }

    pub async fn clone_machine(
        &self,
        id: &str,
        actor: Actor,
        name: &str,
    ) -> anyhow::Result<Machine> {
        let _lock = self.lock(id)?;
        let mut machine = self.machine(id, actor)?;
        if name.trim().is_empty() || name.len() > 100 {
            bail!("Choose a VM name of 1–100 characters");
        }
        self.require_stopped(id).await?;
        machine.id = model::new_uuid();
        machine.name = name.trim().into();
        self.cli()?
            .run(
                id,
                &[
                    "clone".into(),
                    id.into(),
                    machine.id.clone(),
                    "--mount-none".into(),
                ],
                Duration::from_secs(300),
            )
            .await?;
        store::json::write(&self.record(&machine.id)?, &machine)?;
        Ok(machine)
    }

    async fn require_stopped(&self, id: &str) -> anyhow::Result<()> {
        let output = self
            .cli()?
            .output(&["list".into(), "--json".into()], Duration::from_secs(10))
            .await?;
        for line in output.lines().filter(|l| !l.trim().is_empty()) {
            let instance: Instance = serde_json::from_str(line)?;
            if instance.name == id {
                if instance.status == "Stopped" {
                    return Ok(());
                }
                bail!(
                    "Stop this VM before cloning or changing snapshots (current state: {})",
                    instance.status
                );
            }
        }
        bail!("VM instance not found");
    }
}
