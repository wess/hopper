use super::super::files;
use crate::machines::Machines;
use anyhow::{ensure, Context};
use machine::vz::{mac, queue::Check, restore::Image};
use model::{GuestOs, Machine, MachineRuntime};
use serde::{Deserialize, Serialize};
use std::{fs::File, io::Write, os::unix::fs::PermissionsExt, path::PathBuf, sync::Arc};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Platform {
  pub id: String,
  pub version: u32,
  pub disk_gib: u32,
  pub identity: Vec<u8>,
  pub hardware: Vec<u8>,
  pub build: String,
  pub os_version: [isize; 3],
  pub minimum_cpus: usize,
  pub minimum_memory: u64,
}

pub struct Prepared {
  pub(super) machine: Machine,
  pub(super) image: Image,
  pub(super) target: PathBuf,
  pub(super) temporary: Option<tempfile::TempDir>,
  pub(super) runtime: Arc<store::lock::Lease>,
  pub(super) check: Check,
  pub(super) installed: bool,
}

pub fn prepare(
  manager: &Machines,
  machine: Machine,
  image: Image,
  check: Check,
) -> anyhow::Result<Prepared> {
  check()?;
  ensure!(
    manager.root.is_absolute(),
    "Native macOS root must be absolute"
  );
  crate::machines::validate_id(&machine.id)?;
  ensure!(
    machine.guest == GuestOs::Macos
      && machine.profile == "macos"
      && machine.runtime == Some(MachineRuntime::Virtualization),
    "Platform preparation requires native macOS"
  );
  crate::machines::config::validate_resources(&machine)?;
  ensure!(
    !image.hardware.is_empty()
      && image.hardware.len() <= 65536
      && !image.build.is_empty()
      && image.build.len() <= 128
      && image.minimum_cpus > 0
      && image.minimum_memory > 0
      && machine.resources.cpus as usize >= image.minimum_cpus
      && u64::from(machine.resources.memory_gib) << 30 >= image.minimum_memory,
    "Invalid or unsupported macOS restore requirements"
  );
  ensure!(
    !manager.root.join("lima").join(&machine.id).try_exists()?,
    "Previous VM requires migration; its disk is preserved"
  );
  let runtime = Arc::new(manager.guard(&machine.id, ".runtime")?);
  let parent = manager.root.join("vz");
  files::parent(&parent)?;
  let target = parent.join(&machine.id);
  let mut installed = false;
  let temporary = match std::fs::symlink_metadata(&target) {
    Ok(_) => {
      validate(&target, &machine, &image)?;
      let phase = super::deployment::read(&target, &machine.id)?;
      ensure!(
        phase != Some(super::deployment::Phase::Installing),
        "macOS installation requires recovery; its disk is preserved"
      );
      ensure!(
        phase.is_some() || !super::deployment::written(&target)?,
        "Written macOS disk requires recovery; its data is preserved"
      );
      installed = phase == Some(super::deployment::Phase::Installed);
      None
    }
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
      let stage = tempfile::Builder::new().prefix("mac").tempdir_in(&parent)?;
      std::fs::set_permissions(stage.path(), std::fs::Permissions::from_mode(0o700))?;
      let disk = files::write(&stage.path().join("disk"))?;
      disk.set_len(u64::from(machine.resources.disk_gib) << 30)?;
      disk.sync_all()?;
      Some(stage)
    }
    Err(error) => return Err(error.into()),
  };
  check()?;
  Ok(Prepared {
    machine,
    image,
    target,
    temporary,
    runtime,
    check,
    installed,
  })
}

fn saved(target: &std::path::Path) -> anyhow::Result<Platform> {
  files::directory(target)?;
  Ok(serde_json::from_reader(std::io::Read::take(
    files::read(&target.join("platform"), 512 << 10)?,
    (512 << 10) + 1,
  ))?)
}

pub fn inspect(target: &std::path::Path, machine: &Machine) -> anyhow::Result<Platform> {
  let saved = saved(target)?;
  ensure!(
    machine.resources.cpus as usize >= saved.minimum_cpus
      && u64::from(machine.resources.memory_gib) << 30 >= saved.minimum_memory,
    "macOS resources are below the installed platform's requirements"
  );
  let image = Image {
    url: String::new(),
    build: saved.build.clone(),
    version: saved.os_version,
    hardware: saved.hardware.clone(),
    minimum_cpus: saved.minimum_cpus,
    minimum_memory: saved.minimum_memory,
  };
  validate(target, machine, &image)
}

pub fn system(manager: &Machines, machine: Machine, check: Check) -> anyhow::Result<Prepared> {
  check()?;
  ensure!(
    manager.root.is_absolute(),
    "Native macOS root must be absolute"
  );
  crate::machines::validate_id(&machine.id)?;
  let platform = inspect(&manager.root.join("vz").join(&machine.id), &machine)?;
  let image = Image {
    url: String::new(),
    build: platform.build,
    version: platform.os_version,
    hardware: platform.hardware,
    minimum_cpus: platform.minimum_cpus,
    minimum_memory: platform.minimum_memory,
  };
  let prepared = prepare(manager, machine, image, check)?;
  ensure!(prepared.installed, "Install macOS before system boot");
  Ok(prepared)
}

pub fn validate(
  target: &std::path::Path,
  machine: &Machine,
  image: &Image,
) -> anyhow::Result<Platform> {
  files::directory(target)?;
  let platform = saved(target)?;
  ensure!(
    platform.version == 1
      && platform.id == machine.id
      && platform.disk_gib == machine.resources.disk_gib
      && !platform.identity.is_empty()
      && platform.identity.len() <= 4096
      && platform.hardware == image.hardware
      && platform.build == image.build
      && platform.os_version == image.version
      && platform.minimum_cpus == image.minimum_cpus
      && platform.minimum_memory == image.minimum_memory,
    "Native macOS platform does not match this VM and restore image"
  );
  let mut disk = files::read(&target.join("disk"), 2048 << 30)?;
  ensure!(
    disk.metadata()?.len() == u64::from(platform.disk_gib) << 30,
    "Native macOS disk capacity changed; preserve it for recovery"
  );
  let mut magic = [0; 4];
  std::io::Read::read_exact(&mut disk, &mut magic)?;
  ensure!(
    &magic != b"QFI\xfb",
    "Previous disk format requires migration"
  );
  files::directory(&target.join("auxiliary"))?;
  let mut hardware = Vec::new();
  std::io::Read::read_to_end(
    &mut std::io::Read::take(
      files::read(&target.join("auxiliary/hardware"), 65536)?,
      65537,
    ),
    &mut hardware,
  )?;
  ensure!(
    hardware == image.hardware,
    "Auxiliary storage belongs to a different macOS hardware model"
  );
  files::read(&target.join("auxiliary/state"), 128 << 20)?;
  Ok(platform)
}

impl Prepared {
  pub fn directory(&self) -> &std::path::Path {
    self
      .temporary
      .as_ref()
      .map_or(self.target.as_path(), |stage| stage.path())
  }

  pub fn publish(&mut self, main: machine::vz::MainThreadMarker) -> anyhow::Result<Platform> {
    let _main = main;
    (self.check)()?;
    if let Some(stage) = self.temporary.take() {
      mac::create_auxiliary(&stage.path().join("auxiliary"), &self.image.hardware)?;
      let platform = Platform {
        id: self.machine.id.clone(),
        version: 1,
        disk_gib: self.machine.resources.disk_gib,
        identity: mac::identity(),
        hardware: self.image.hardware.clone(),
        build: self.image.build.clone(),
        os_version: self.image.version,
        minimum_cpus: self.image.minimum_cpus,
        minimum_memory: self.image.minimum_memory,
      };
      let mut file = files::write(&stage.path().join("platform"))?;
      serde_json::to_writer(&mut file, &platform)?;
      file.flush()?;
      file.sync_all()?;
      File::open(stage.path())?.sync_all()?;
      (self.check)()?;
      files::publish(stage, &self.target)?;
    }
    (self.check)()?;
    validate(&self.target, &self.machine, &self.image)
      .context("Validate native macOS state before admission")
  }
}
