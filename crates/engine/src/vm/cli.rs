use anyhow::{bail, Context};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
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
        let output = self
            .output(&["list".into(), "--json".into()], Duration::from_secs(5))
            .await?;
        parse_instances(&output)
    }

    pub async fn run(&self, args: &[String], timeout: Duration) -> anyhow::Result<()> {
        self.execute(args, timeout, None, true).await.map(|_| ())
    }

    pub async fn output(&self, args: &[String], timeout: Duration) -> anyhow::Result<String> {
        self.execute(args, timeout, None, false).await
    }

    pub async fn input(
        &self,
        args: &[String],
        input: &[u8],
        timeout: Duration,
    ) -> anyhow::Result<String> {
        self.execute(args, timeout, Some(input), false).await
    }

    async fn execute(
        &self,
        args: &[String],
        timeout: Duration,
        input: Option<&[u8]>,
        logging: bool,
    ) -> anyhow::Result<String> {
        ensure_home(&self.home).await?;
        let log_path = self.home.join("engine.log");
        let log = if logging {
            Some(Arc::new(tokio::sync::Mutex::new(
                super::log::Log::open(log_path.clone()).await?,
            )))
        } else {
            None
        };
        let mut child = self
            .command(args)
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Could not launch Hopper's VM helper")?;
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let mut stdin = child.stdin.take();
        let result = tokio::time::timeout(timeout, async {
            tokio::try_join!(
                pump(stdout, log.clone()),
                pump(stderr, log),
                async {
                    if let (Some(input), Some(stdin)) = (input, stdin.as_mut()) {
                        stdin.write_all(input).await?;
                        stdin.shutdown().await?;
                    }
                    drop(stdin.take());
                    Ok::<_, anyhow::Error>(())
                },
                async { Ok::<_, anyhow::Error>(child.wait().await?) }
            )
        })
        .await
        .with_context(|| format!("VM operation timed out. See {}", log_path.display()))??;
        let (stdout, stderr, (), status) = result;
        if !status.success() {
            if logging {
                bail!("VM operation failed ({status}). See {}", log_path.display());
            }
            // Input can contain proxy credentials. Never reflect subprocess
            // output for an operation that received secret-bearing stdin.
            if input.is_some() {
                bail!(
                    "VM configuration operation failed ({status}). Check Docker daemon settings."
                );
            }
            bail!(
                "VM operation failed ({status}): {}",
                String::from_utf8_lossy(&stderr)
            );
        }
        Ok(String::from_utf8_lossy(&stdout).into())
    }
}

async fn pump(
    mut input: impl AsyncRead + Unpin,
    log: Option<Arc<tokio::sync::Mutex<super::log::Log>>>,
) -> anyhow::Result<Vec<u8>> {
    let mut result = Vec::new();
    let mut chunk = [0; 8192];
    loop {
        let count = input.read(&mut chunk).await?;
        if count == 0 {
            break;
        }
        if let Some(log) = &log {
            log.lock().await.write(&chunk[..count]).await?;
        } else {
            anyhow::ensure!(
                result.len() + count <= 64 * 1024,
                "VM helper output exceeded 64 KiB."
            );
            result.extend_from_slice(&chunk[..count]);
        }
    }
    Ok(result)
}

pub async fn ensure_home(home: &Path) -> anyhow::Result<()> {
    tokio::fs::create_dir_all(home).await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(home, std::fs::Permissions::from_mode(0o700)).await?;
    }
    Ok(())
}

/// OS locks are released after crashes too. UI and MCP processes cannot run
/// competing lifecycle operations against the same VM directory.
pub struct Lifecycle(std::fs::File);

impl Drop for Lifecycle {
    fn drop(&mut self) {
        // Explicit unlock also releases the lock when a concurrently spawned
        // child briefly inherited the descriptor before its exec closes it.
        let _ = self.0.unlock();
    }
}

pub async fn lifecycle(home: &Path) -> anyhow::Result<Lifecycle> {
    ensure_home(home).await?;
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(home.join("lifecycle.lock"))?;
    file.try_lock().map_err(|_| {
        anyhow::anyhow!("Another Hopper process is changing this VM. Wait for it to finish.")
    })?;
    Ok(Lifecycle(file))
}
