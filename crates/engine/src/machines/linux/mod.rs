pub mod boot;
pub mod download;
mod files;
pub mod iso;
pub mod kernel;
pub mod observe;
pub mod preparation;
pub mod progress;
pub mod provision;
pub mod records;
pub mod seed;

use anyhow::ensure;
use machine::vz::queue::Check;
use std::{
  path::{Path, PathBuf},
  time::Duration,
};

pub const URL: &str =
  "https://cdimage.ubuntu.com/ubuntu/releases/24.04/release/ubuntu-24.04.5-desktop-arm64.iso";
pub const SHA256: &str = "2be09ca883921bff6d8e6b0bfbafd13e32436553b7086f33bce3a4c5bad8bd14";
pub const SIZE: u64 = 3_967_463_424;

pub async fn prepare(root: &Path, check: Check) -> anyhow::Result<PathBuf> {
  let (progress, _) = tokio::sync::watch::channel(preparation::Phase::Inspecting);
  prepare_tracked(root, check, progress).await
}

pub async fn prepare_tracked(
  root: &Path,
  check: Check,
  progress: tokio::sync::watch::Sender<preparation::Phase>,
) -> anyhow::Result<PathBuf> {
  check()?;
  ensure!(root.is_absolute(), "Linux media root must be absolute");
  let mut cache = root.to_owned();
  for part in ["native", "images", "linux"] {
    cache.push(part);
    files::directory(&cache)?;
  }
  let client = reqwest::Client::builder()
    .connect_timeout(Duration::from_secs(30))
    .read_timeout(Duration::from_secs(60))
    .redirect(reqwest::redirect::Policy::none())
    .build()?;
  download::tracked(&client, URL, SIZE, SHA256, &cache, check, &progress).await
}
