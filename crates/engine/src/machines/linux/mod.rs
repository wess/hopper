pub mod download;
pub mod boot;
pub mod iso;
pub mod kernel;
mod files;
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
  download::fetch(&client, URL, SIZE, SHA256, &cache, check).await
}
