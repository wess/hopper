use super::{download, Phase};
use crate::machines::{vz::files, Machines};
use anyhow::ensure;
use machine::vz::{
  queue::Check,
  restore::{self, Image},
};
use std::{
  path::PathBuf,
  sync::Arc,
  time::{Duration, Instant},
};
use tokio::sync::watch::Sender;

pub async fn latest(check: Check) -> anyhow::Result<Image> {
  check()?;
  let discovery = restore::latest_owned(Arc::new(check.clone()));
  let deadline = Instant::now() + Duration::from_secs(120);
  loop {
    check()?;
    if let Some(result) = restore::poll(&discovery)? {
      return result;
    }
    ensure!(
      Instant::now() < deadline,
      "Supported macOS restore discovery timed out"
    );
    tokio::time::sleep(Duration::from_millis(25)).await;
  }
}

pub fn matches(expected: &Image, local: &Image) -> anyhow::Result<()> {
  ensure!(
    expected.build == local.build
      && expected.version == local.version
      && expected.hardware == local.hardware
      && expected.minimum_cpus == local.minimum_cpus
      && expected.minimum_memory == local.minimum_memory,
    "Downloaded restore image does not match supported Apple metadata"
  );
  Ok(())
}

pub async fn fetch(
  manager: &Machines,
  machine: &model::Machine,
  check: Check,
  progress: &Sender<Phase>,
) -> anyhow::Result<(PathBuf, Image)> {
  progress.send_replace(Phase::Discovering);
  let image = latest(check.clone()).await?;
  check()?;
  ensure!(
    machine.resources.cpus as usize >= image.minimum_cpus
      && u64::from(machine.resources.memory_gib) << 30 >= image.minimum_memory,
    "macOS resources are below the supported restore image's requirements"
  );
  let client = reqwest::Client::builder()
    .redirect(reqwest::redirect::Policy::none())
    .connect_timeout(Duration::from_secs(30))
    .read_timeout(Duration::from_secs(60))
    .build()?;
  let source = download::checked(&check, download::inspect(&client, &image.url)).await??;
  check()?;
  let images = manager.root.join("images");
  files::parent(&images)?;
  let root = images.join("macos");
  files::parent(&root)?;
  let path = download::fetch(&client, &source, &root, check.clone(), progress).await?;
  check()?;
  Ok((path, image))
}
