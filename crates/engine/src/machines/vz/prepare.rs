mod admit;

use super::{files, identity::validate, Client};
use crate::machines::{native::assets, Machines};
use anyhow::{ensure, Context};
use machine::vz::{self, queue::Check};
use model::Machine;
use std::{
  io::{Read, Seek, SeekFrom, Write},
  os::unix::fs::{MetadataExt, PermissionsExt},
  path::PathBuf,
  sync::Arc,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
  Launch,
  Installer,
  Unattended,
  System,
}

pub struct Prepared {
  machine: Machine,
  manager: Machines,
  actor: crate::machines::Actor,
  intent: Option<String>,
  target: PathBuf,
  temporary: Option<tempfile::TempDir>,
  installer: Option<PathBuf>,
  seed: Option<tempfile::TempDir>,
  media: Option<tempfile::TempDir>,
  unattended: bool,
  attempt: Option<String>,
  runtime: Arc<store::lock::Lease>,
  network: vz::network::Mode,
  shares: Vec<vz::sharing::Directory>,
  check: Check,
  client: Client,
}

pub struct Admission {
  id: String,
  check: Check,
  client: Client,
  installation: Option<super::Installation>,
}

pub(super) fn prepare(
  manager: Machines,
  machine: Machine,
  check: Check,
  client: Client,
  stage: Stage,
) -> anyhow::Result<Prepared> {
  check()?;
  crate::machines::config::validate_resources(&machine)?;
  ensure!(manager.root.is_absolute(), "VZ root must be absolute");
  ensure!(
    uuid::Uuid::parse_str(&machine.id)?.to_string() == machine.id,
    "VZ identity must be canonical"
  );
  ensure!(
    stage != Stage::Launch,
    "Resolve Linux launch stage before preparation"
  );
  let installer = match stage {
    Stage::Installer | Stage::Unattended => {
      let path = PathBuf::from(
        machine
          .installer
          .as_deref()
          .context("Acquire Linux installation media before native installation")?,
      );
      ensure!(path.is_absolute(), "Linux installer must be absolute");
      let mut media = assets::regular(&path, 16 << 30)?;
      media.seek(SeekFrom::Start(32768))?;
      let mut header = [0; 7];
      media.read_exact(&mut header)?;
      ensure!(
        &header[1..6] == b"CD001",
        "Linux installer must contain an ISO volume descriptor"
      );
      Some(path)
    }
    Stage::System => None,
    Stage::Launch => unreachable!(),
  };
  let runtime = Arc::new(manager.guard(&machine.id, ".runtime")?);
  let network = super::network::mode(&manager, &machine.id)?;
  let shares = super::sharing::directories(&manager, &machine.id)?;
  let parent = manager.root.join("vz");
  files::parent(&parent)?;
  let target = parent.join(&machine.id);
  let temporary = match std::fs::symlink_metadata(&target) {
    Ok(_) => {
      files::directory(&target)?;
      validate(&target, &machine)?;
      None
    }
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
      ensure!(
        matches!(stage, Stage::Installer | Stage::Unattended),
        "Prepare Linux installation before system boot"
      );
      let temporary = tempfile::Builder::new()
        .prefix("linux")
        .tempdir_in(&parent)?;
      std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o700))?;
      let disk = files::write(&temporary.path().join("disk"))?;
      disk.set_len(u64::from(machine.resources.disk_gib) << 30)?;
      disk.sync_all()?;
      Some(temporary)
    }
    Err(error) => return Err(error.into()),
  };
  if matches!(stage, Stage::System) {
    ensure!(
      files::read(&target.join("disk"), 2048 << 30)?
        .metadata()?
        .blocks()
        > 0,
      "Linux system disk has no installation data"
    );
  }
  if matches!(stage, Stage::Unattended) {
    ensure!(
      machine.profile == "ubuntu" && machine.runtime == Some(model::MachineRuntime::Virtualization),
      "Unattended installation requires native Ubuntu"
    );
    let disk = temporary.as_ref().map_or_else(
      || target.join("disk"),
      |directory| directory.path().join("disk"),
    );
    ensure!(
      files::read(&disk, 2048 << 30)?.metadata()?.blocks() == 0,
      "This VM disk contains data; preserve it for installation recovery or system boot"
    );
  }
  check()?;
  Ok(Prepared {
    machine,
    manager,
    actor: crate::machines::Actor::Person,
    intent: None,
    target,
    temporary,
    installer,
    seed: None,
    media: None,
    unattended: false,
    attempt: None,
    runtime,
    network,
    shares,
    check,
    client,
  })
}

impl Prepared {
  pub(super) fn control(mut self, actor: crate::machines::Actor, intent: Option<String>) -> Self {
    self.actor = actor;
    self.intent = intent;
    self
  }
  pub fn stage(&self) -> Stage {
    if self.installer.is_none() {
      Stage::System
    } else if self.unattended {
      Stage::Unattended
    } else {
      Stage::Installer
    }
  }

  pub(super) fn unattended_tracked(
    mut self,
    manager: &Machines,
    actor: crate::machines::Actor,
    progress: &tokio::sync::watch::Sender<crate::machines::linux::preparation::Phase>,
  ) -> anyhow::Result<Self> {
    use crate::machines::linux::preparation::Phase;
    (self.check)()?;
    progress.send_replace(Phase::Media);
    let installer = self
      .installer
      .as_deref()
      .context("Unattended installation requires media")?;
    let directory = crate::machines::linux::boot::stage(
      installer,
      self.target.parent().context("VZ state needs a parent")?,
      &self.check,
    )?;
    (self.check)()?;
    progress.send_replace(Phase::Accounts);
    let credentials =
      crate::machines::linux::provision::persisted(manager, &self.machine.id, actor)?;
    (self.check)()?;
    let attempt = uuid::Uuid::new_v4().to_string();
    let plan =
      crate::machines::linux::provision::tracked(&self.machine.id, &credentials, &attempt)?;
    self.attempt = Some(attempt);
    self = self.provision(&plan)?;
    self.installer = Some(directory.path().join("installer"));
    self.media = Some(directory);
    self.unattended = true;
    (self.check)()?;
    Ok(self)
  }

  pub fn provision(
    mut self,
    plan: &crate::machines::linux::provision::Plan,
  ) -> anyhow::Result<Self> {
    (self.check)()?;
    ensure!(
      self.installer.is_some(),
      "Provisioning media requires an installer boot"
    );
    ensure!(
      self.seed.is_none(),
      "Provisioning media is already attached"
    );
    let image = crate::machines::linux::seed::image(plan)?;
    let directory = tempfile::Builder::new()
      .prefix("seed")
      .tempdir_in(self.target.parent().context("VZ state needs a parent")?)?;
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
    let mut file = files::write(&directory.path().join("seed"))?;
    file.write_all(&image)?;
    file.sync_all()?;
    (self.check)()?;
    self.seed = Some(directory);
    Ok(self)
  }
}

impl Admission {
  pub fn installation(&self) -> Option<super::Installation> {
    self.installation.clone()
  }
  pub async fn start(self) -> anyhow::Result<()> {
    self
      .client
      .transition_checked(&self.id, vz::Action::Start, self.check)
      .await
  }
}
