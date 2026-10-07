//! Read-only installer inspection without extracting the multi-gigabyte install image.

use super::{deploy::Layout, image, setup::process};
use anyhow::{ensure, Context};
use std::{
  path::{Path, PathBuf},
  process::Stdio,
  time::{Duration, Instant},
};

pub struct Contents {
  pub image_index: u32,
  pub recovery_image_bytes: u64,
}

pub fn layout(contents: &Contents, disk_gib: u32) -> anyhow::Result<Layout> {
  let recovery_mib = contents.recovery_image_bytes.div_ceil(1024 * 1024) + 314;
  let layout = Layout {
    disk_gib,
    image_index: contents.image_index,
    recovery_mib: u32::try_from(recovery_mib.max(1024))?,
    recovery_image_bytes: contents.recovery_image_bytes,
  };
  super::deploy::prepare(&layout)?;
  Ok(layout)
}

pub fn recovery_size(text: &str) -> anyhow::Result<u64> {
  ensure!(text.len() <= 64 * 1024, "Oversized recovery image metadata");
  let mut values = text
    .lines()
    .filter_map(|line| line.strip_prefix("Uncompressed size = "));
  let value = values.next().context("Missing recovery image size")?;
  ensure!(values.next().is_none(), "Ambiguous recovery image size");
  let size = value
    .strip_suffix(" bytes")
    .context("Invalid recovery image size unit")?
    .parse()?;
  ensure!(
    (1..=3 * 1024 * 1024 * 1024).contains(&size),
    "Invalid recovery image size"
  );
  Ok(size)
}

pub async fn read(installer: &Path, wim: &Path, mount_tool: &Path) -> anyhow::Result<Contents> {
  ensure!(
    installer.is_absolute() && wim.is_absolute() && mount_tool.is_absolute(),
    "Inspector paths must be absolute"
  );
  let info = std::fs::symlink_metadata(installer)?;
  ensure!(
    info.is_file() && (1..=12 * 1024 * 1024 * 1024).contains(&info.len()),
    "Invalid installer file"
  );
  let mount = Mount::new(mount_tool)?;
  let result = async {
    process::run(
      mount_tool,
      &[
        "image".into(),
        "attach".into(),
        "--readOnly".into(),
        "--nobrowse".into(),
        "--mountPoint".into(),
        text(&mount.point)?,
        "--plist".into(),
        text(installer)?,
      ],
      None,
      None,
      1024 * 1024,
    )
    .await
    .context("Mount Windows installer read-only")?;
    let image = mount.point.join("sources/install.wim");
    let info = std::fs::symlink_metadata(&image)?;
    ensure!(
      info.is_file() && (1..=12 * 1024 * 1024 * 1024).contains(&info.len()),
      "Installer has no supported install.wim"
    );
    let xml = process::run(
      wim,
      &["info".into(), text(&image)?, "--xml".into()],
      None,
      None,
      1024 * 1024,
    )
    .await?;
    let image_index = image::professional(&xml)?;
    let listing = process::run(
      wim,
      &[
        "dir".into(),
        text(&image)?,
        image_index.to_string(),
        "--path=/Windows/System32/Recovery".into(),
      ],
      None,
      None,
      64 * 1024,
    )
    .await?;
    let listing = std::str::from_utf8(&listing)?;
    let mut paths = listing
      .lines()
      .filter(|path| path.eq_ignore_ascii_case("/Windows/System32/Recovery/winre.wim"));
    let recovery = paths
      .next()
      .context("Installer has no Windows recovery image")?;
    ensure!(paths.next().is_none(), "Ambiguous recovery image path");
    let metadata = process::run(
      wim,
      &[
        "dir".into(),
        text(&image)?,
        image_index.to_string(),
        format!("--path={recovery}"),
        "--detailed".into(),
        "--one-file-only".into(),
      ],
      None,
      None,
      64 * 1024,
    )
    .await?;
    Ok::<_, anyhow::Error>(Contents {
      image_index,
      recovery_image_bytes: recovery_size(std::str::from_utf8(&metadata)?)?,
    })
  }
  .await;
  // cleanup outlives caller cancellation and does not traverse mounted image contents.
  let cleanup = mount.close().await;
  let contents = result?;
  cleanup?;
  Ok(contents)
}

fn text(path: &Path) -> anyhow::Result<String> {
  Ok(
    path
      .to_str()
      .context("Installer path is not UTF-8")?
      .to_owned(),
  )
}

struct Mount {
  root: Option<PathBuf>,
  point: PathBuf,
  tool: PathBuf,
}

impl Mount {
  fn new(tool: &Path) -> anyhow::Result<Self> {
    let root = tempfile::tempdir()?.keep();
    Ok(Self {
      point: root.join("media"),
      root: Some(root),
      tool: tool.to_owned(),
    })
  }

  async fn close(mut self) -> anyhow::Result<()> {
    let root = self.root.take().unwrap();
    let point = self.point.clone();
    let tool = self.tool.clone();
    tokio::task::spawn_blocking(move || eject(&tool, &point, &root)).await?
  }
}

impl Drop for Mount {
  fn drop(&mut self) {
    if let Some(root) = self.root.take() {
      let point = self.point.clone();
      let tool = self.tool.clone();
      std::thread::spawn(move || {
        let _ = eject(&tool, &point, &root);
      });
    }
  }
}

fn eject(tool: &Path, point: &Path, root: &Path) -> anyhow::Result<()> {
  let mut child = std::process::Command::new(tool)
    .arg("eject")
    .arg(point)
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn()?;
  let deadline = Instant::now() + Duration::from_secs(60);
  let status = loop {
    if let Some(status) = child.try_wait()? {
      break status;
    }
    if Instant::now() >= deadline {
      let _ = child.kill();
      let _ = child.wait();
      anyhow::bail!("Installer ejection timed out; private mount directory retained");
    }
    std::thread::sleep(Duration::from_millis(20));
  };
  ensure!(
    status.success(),
    "Installer ejection failed; private mount directory retained"
  );
  // macOS removes its mount point after ejection. Remove only empty directories.
  if point.exists() {
    std::fs::remove_dir(point)?;
  }
  std::fs::remove_dir(root)?;
  Ok(())
}
