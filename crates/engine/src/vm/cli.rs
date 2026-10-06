use anyhow::{bail, Context};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

use super::config::INSTANCE;

#[derive(Clone)]
pub struct Cli {
    pub bin: PathBuf,
    pub home: PathBuf,
}

#[derive(Debug, Deserialize)]
pub struct Instance {
    pub name: String,
    pub status: String,
}

pub fn parse_instances(raw: &str) -> anyhow::Result<Option<Instance>> {
    for line in raw.lines().filter(|line| !line.trim().is_empty()) {
        let instance: Instance = serde_json::from_str(line)?;
        if instance.name == INSTANCE {
            return Ok(Some(instance));
        }
    }
    Ok(None)
}

pub fn locate() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("HOPPER_LIMA_BIN") {
        let path = PathBuf::from(path);
        return executable(&path).then_some(path);
    }
    let exe = std::env::current_exe().ok()?;
    let bundled = bundle_paths(&exe);
    let dev = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../native/build/lima/bin/limactl");
    bundled
        .into_iter()
        .chain([dev, PathBuf::from("/opt/homebrew/bin/limactl")])
        .chain(std::env::var_os("PATH").into_iter().flat_map(|paths| {
            std::env::split_paths(&paths)
                .map(|p| p.join("limactl"))
                .collect::<Vec<_>>()
        }))
        .find(|p| executable(p))
}

pub fn bundle_paths(exe: &Path) -> Vec<PathBuf> {
    // the app and the MCP sidecar live at different depths inside Contents
    exe.ancestors()
        .skip(1)
        .take(4)
        .map(|dir| dir.join("Resources/lima/bin/limactl"))
        .collect()
}

fn executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    path.is_file()
}

impl Cli {
    fn command(&self, args: &[String]) -> Command {
        let mut command = Command::new(&self.bin);
        command
            .args(["--tty=false"])
            .args(args)
            .env("LIMA_HOME", &self.home)
            .env_remove("LIMA_INSTANCE")
            .env_remove("LIMA_SHARE_PATH")
            .env_remove("LIMA_TEMPLATES_PATH")
            .env_remove("LIMA_DRIVERS_PATH")
            .stdin(Stdio::null())
            .kill_on_drop(true);
        command
    }

    pub async fn instance(&self) -> anyhow::Result<Option<Instance>> {
        if !self.home.join(INSTANCE).exists() {
            return Ok(None);
        }
        let output = tokio::time::timeout(
            Duration::from_secs(5),
            self.command(&["list".into(), "--json".into()]).output(),
        )
        .await
        .context("VM status check timed out")??;
        if !output.status.success() {
            bail!(
                "VM status check failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        parse_instances(&String::from_utf8_lossy(&output.stdout))
    }

    pub async fn run(&self, args: &[String], timeout: Duration) -> anyhow::Result<()> {
        tokio::fs::create_dir_all(&self.home).await?;
        let log_path = self.home.join("engine.log");
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?;
        let mut child = self
            .command(args)
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log))
            .spawn()
            .context("Could not launch Hopper's VM helper")?;
        let status = tokio::time::timeout(timeout, child.wait())
            .await
            .with_context(|| format!("VM operation timed out. See {}", log_path.display()))??;
        if !status.success() {
            bail!("VM operation failed ({status}). See {}", log_path.display());
        }
        Ok(())
    }
}
