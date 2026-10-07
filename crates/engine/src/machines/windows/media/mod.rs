pub mod cache;
pub mod convert;

use super::{catalog, setup::Tools};
use crate::machines::media::download;
use anyhow::{ensure, Context};
use reqwest::{redirect, Client};
use std::{
  path::{Path, PathBuf},
  time::Duration,
};

#[derive(Clone, Copy, Debug)]
pub enum Phase {
  VerifyingCache,
  Catalogue,
  Download,
  Convert,
  Publish,
}

pub fn installer(root: &Path) -> PathBuf {
  root.join("native/images/windows/installer/installer.iso")
}

pub async fn prepare(
  root: &Path,
  tools: &Tools,
  progress: &Path,
  mut phase: impl FnMut(Phase),
) -> anyhow::Result<PathBuf> {
  ensure!(root.is_absolute(), "Native media root must be absolute");
  let mut folder = root.to_owned();
  for part in ["native", "images", "windows"] {
    folder.push(part);
    cache::directory(&folder)?;
  }
  let mut options = std::fs::OpenOptions::new();
  options.create(true).truncate(false).read(true).write(true);
  #[cfg(unix)]
  {
    use std::os::unix::fs::OpenOptionsExt;
    options
      .mode(0o600)
      .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
  }
  let lock = options.open(folder.join("lock"))?;
  ensure!(lock.metadata()?.is_file(), "Invalid native media lease");
  let _lease = store::lock::exclusive(lock)
    .context("Another VM is preparing Windows media; retry when it finishes")?;
  let complete = folder.join("installer");
  match std::fs::symlink_metadata(&complete) {
    Ok(_) => {
      phase(Phase::VerifyingCache);
      tokio::fs::write(progress, "Verifying cached Windows installation media…").await?;
      return cache::verify(&complete, &tools.archive).await;
    }
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
    Err(error) => return Err(error.into()),
  }
  ensure!(
    fs2::available_space(&folder)? >= 20 * 1024 * 1024 * 1024,
    "Windows media preparation needs 20 GiB of free disk space"
  );
  phase(Phase::Catalogue);
  tokio::fs::write(progress, "Fetching official Windows installer catalogue…").await?;
  let selection = catalog::fetch(&tools.archive).await?;
  let client = Client::builder()
    .connect_timeout(Duration::from_secs(30))
    .read_timeout(Duration::from_secs(60))
    .redirect(redirect::Policy::none())
    .build()?;
  phase(Phase::Download);
  let esd = download::fetch(&client, &selection.media, &folder, progress).await?;
  phase(Phase::Convert);
  tokio::fs::write(progress, "Converting verified Windows installer…").await?;
  let stage = tempfile::tempdir_in(&folder)?;
  let bundle = stage.path().join("installer");
  cache::directory(&bundle)?;
  convert::build(&esd, &bundle.join("installer.iso"), tools).await?;
  phase(Phase::Publish);
  tokio::fs::write(progress, "Verifying prepared Windows installation media…").await?;
  let iso = cache::publish(&bundle, &complete, &selection, &tools.archive).await?;
  // retain the verified ESD through conversion failures; the completed ISO supersedes it.
  let _ = tokio::fs::remove_file(esd).await;
  Ok(iso)
}
