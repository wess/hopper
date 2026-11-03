use super::{assets, boot, paths, validate, Assets};
use crate::machines::{
  native::{
    installation,
    restore::{Owner, Phase},
  },
  Actor, Machines,
};
use anyhow::{ensure, Context};
use model::native::Boot;

pub(crate) fn prepare(
  manager: &Machines,
  id: &str,
  assets: &Assets,
  owner: &Owner,
) -> anyhow::Result<Boot> {
  owner.namespace(manager, id)?;
  let selected = owner
    .selected()?
    .context("No active generation selection")?;
  ensure!(
    selected.phase == Phase::Active,
    "Native restore requires recovery before startup"
  );
  let machine = manager.machine(id, Actor::Person)?;
  validate(&machine)?;
  let installed = installation::read(manager, id, Actor::Person)?;
  ensure!(
    installed.is_some()
      && matches!(
        installation::boot_stage(installed.as_ref())?,
        super::Stage::System
      ),
    "Active generation requires completed Windows deployment"
  );
  let folder = assets
    .firmware
    .parent()
    .context("Native firmware directory is missing")?;
  let assets = assets::verify(&assets.worker, folder)?;
  let paths = paths(&manager.root, id)?;
  let boot = boot(id, &machine, &assets, &paths, None, None)?;
  owner.namespace(manager, id)?;
  ensure!(
    owner.selected()? == Some(selected),
    "Active generation selection changed"
  );
  Ok(boot)
}
