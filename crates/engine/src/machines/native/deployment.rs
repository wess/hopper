//! Preparation runs under the session's operation and runtime ownership locks.

use super::{assets::Assets, config};
use crate::machines::{
  windows::{deploy, inspect, provision, setup},
  Actor, Machines,
};
use anyhow::{ensure, Context};
use std::path::{Path, PathBuf};
use tokio::sync::watch;

#[derive(Clone)]
pub struct Tools {
  pub media: setup::Tools,
  pub mount: PathBuf,
  pub drivers: PathBuf,
  pub license: PathBuf,
}

#[derive(Clone, Copy, Debug)]
pub enum Phase {
  Inspecting,
  Accounts,
  Media(setup::Phase),
  Disk,
  Starting,
}

pub(super) async fn prepare(
  manager: &Machines,
  id: &str,
  assets: &Assets,
  tools: &Tools,
  progress: &watch::Sender<Phase>,
) -> anyhow::Result<model::native::Boot> {
  super::assets::verify(
    &assets.worker,
    assets
      .firmware
      .parent()
      .context("Missing firmware directory")?,
  )?;
  let paths = config::initialize(manager, id)?;
  let record = manager.machine(id, Actor::Person)?;
  let disk = if std::fs::symlink_metadata(&paths.disk).is_ok() {
    Some(config::target(&paths, &record, true)?)
  } else {
    None
  };
  let installer = record
    .installer
    .as_deref()
    .context("Prepare Windows installation media before native deployment")?;
  let installer = Path::new(installer);
  ensure!(installer.is_absolute(), "Installer path must be absolute");
  let complete = std::fs::symlink_metadata(&paths.setup).is_ok();
  if complete {
    config::verify_setup(&paths.setup, id)?;
  }
  progress.send_replace(Phase::Inspecting);
  let contents = inspect::read(installer, &tools.media.wim, &tools.mount).await?;
  let layout = inspect::layout(&contents, record.resources.disk_gib)?;
  let plan = deploy::prepare(&layout)?;
  if complete {
    setup::verify_source(&paths.setup, installer, &plan).await?;
  }
  if !complete {
    progress.send_replace(Phase::Accounts);
    let accounts = provision::persisted(manager, id, Actor::Person)?;
    let provision = provision::prepare(id, &accounts)?;
    setup::build(
      &setup::Input {
        vm_id: id,
        installer,
        drivers: &tools.drivers,
        license: &tools.license,
        output: &paths.setup,
        plan: &plan,
        provision: &provision,
      },
      &tools.media,
      |phase| {
        progress.send_replace(Phase::Media(phase));
      },
    )
    .await?;
  }
  progress.send_replace(Phase::Disk);
  if disk.is_none() {
    drop(deploy::create_disk(&paths.disk, &layout)?);
  }
  drop(disk);
  let boot = config::prepare(manager, id, assets, config::Stage::Deployment)?;
  progress.send_replace(Phase::Starting);
  Ok(boot)
}
