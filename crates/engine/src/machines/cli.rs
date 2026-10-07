use anyhow::{bail, Context};
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;

async fn capture(reader: impl AsyncRead + Unpin, limit: u64) -> anyhow::Result<Vec<u8>> {
  let mut bytes = Vec::new();
  reader.take(limit + 1).read_to_end(&mut bytes).await?;
  if bytes.len() as u64 > limit {
    bail!("Guest output exceeded {} bytes", limit);
  }
  Ok(bytes)
}

#[derive(Clone)]
pub struct Cli {
  pub bin: PathBuf,
  pub home: PathBuf,
}

pub fn media_paths() -> Vec<PathBuf> {
  if let Ok(exe) = std::env::current_exe() {
    if let Some(bundled) = exe
      .ancestors()
      .skip(1)
      .take(4)
      .map(|dir| dir.join("Resources/windows/bin"))
      .find(|dir| dir.join("wimlib-imagex").is_file())
    {
      return vec![bundled];
    }
  }
  let mut paths = vec![
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../native/build/windows/bin"),
    PathBuf::from("/opt/homebrew/bin"),
  ];
  paths.extend(
    std::env::var_os("PATH")
      .into_iter()
      .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>()),
  );
  paths
}

impl Cli {
  pub fn command(&self, args: &[String]) -> Command {
    let mut command = Command::new(&self.bin);
    command
      .args(["--tty=false"])
      .args(args)
      .env("LIMA_HOME", &self.home)
      .env_remove("LIMA_INSTANCE")
      .env_remove("LIMA_SHARE_PATH")
      .env_remove("LIMA_TEMPLATES_PATH")
      .env_remove("LIMA_DRIVERS_PATH")
      .env_remove("QEMU_SYSTEM_AARCH64")
      .stdin(Stdio::null())
      .kill_on_drop(true);
    command.env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin");
    command
  }

  pub async fn output(&self, args: &[String], timeout: Duration) -> anyhow::Result<String> {
    let mut child = self
      .command(args)
      .stdout(Stdio::piped())
      .stderr(Stdio::piped())
      .spawn()?;
    let stdout = child.stdout.take().context("Missing VM stdout")?;
    let stderr = child.stderr.take().context("Missing VM stderr")?;
    let (status, stdout, stderr) = tokio::time::timeout(timeout, async {
      tokio::try_join!(
        async { Ok::<_, anyhow::Error>(child.wait().await?) },
        capture(stdout, 8 * 1024 * 1024),
        capture(stderr, 256 * 1024),
      )
    })
    .await
    .context("VM operation timed out")??;
    if !status.success() {
      bail!("{}", String::from_utf8_lossy(&stderr).trim());
    }
    String::from_utf8(stdout).context("VM helper returned non-UTF8 output")
  }

  pub async fn run(&self, id: &str, args: &[String], timeout: Duration) -> anyhow::Result<()> {
    let log_path = self
      .home
      .parent()
      .context("Missing machine directory")?
      .join("logs")
      .join(format!("{id}.log"));
    std::fs::create_dir_all(log_path.parent().unwrap())?;
    let log = std::fs::OpenOptions::new()
      .create(true)
      .append(true)
      .open(&log_path)?;
    let mut child = self
      .command(args)
      .stdout(Stdio::from(log.try_clone()?))
      .stderr(Stdio::from(log))
      .spawn()?;
    let status = tokio::time::timeout(timeout, child.wait())
      .await
      .with_context(|| format!("VM operation timed out. See {}", log_path.display()))??;
    if !status.success() {
      bail!("VM operation failed ({status}). See {}", log_path.display());
    }
    Ok(())
  }
}
