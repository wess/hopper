use super::{files, Client, Owner};
use crate::machines::{native::assets, Machines};
use anyhow::{ensure, Context};
use machine::vz::{self, queue::Check, MainThreadMarker};
use model::Machine;
use serde::{Deserialize, Serialize};
use std::{
  fs::File,
  io::{Read, Seek, SeekFrom, Write},
  os::unix::fs::{MetadataExt, PermissionsExt},
  path::PathBuf,
  sync::Arc,
};

#[derive(Clone, Copy)]
pub enum Stage {
  Installer,
  System,
}

pub struct Prepared {
  machine: Machine,
  target: PathBuf,
  temporary: Option<tempfile::TempDir>,
  installer: Option<PathBuf>,
  seed: Option<tempfile::TempDir>,
  runtime: Arc<store::lock::Lease>,
  check: Check,
  client: Client,
}

pub struct Admission {
  id: String,
  check: Check,
  client: Client,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Identity {
  id: String,
  version: u32,
  guest: model::GuestOs,
  disk_gib: u32,
  identity: Vec<u8>,
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
  let installer = match stage {
    Stage::Installer => {
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
  };
  let runtime = Arc::new(manager.guard(&machine.id, ".runtime")?);
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
        matches!(stage, Stage::Installer),
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
  check()?;
  Ok(Prepared {
    machine,
    target,
    temporary,
    installer,
    seed: None,
    runtime,
    check,
    client,
  })
}

impl Prepared {
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

  pub fn admit(mut self, main: MainThreadMarker, owner: &mut Owner) -> anyhow::Result<Admission> {
    (self.check)()?;
    if let Some(temporary) = self.temporary.take() {
      vz::create_variables(&temporary.path().join("variables"))?;
      File::open(temporary.path().join("variables"))?.sync_all()?;
      let identity = Identity {
        id: self.machine.id.clone(),
        version: 1,
        guest: self.machine.guest,
        disk_gib: self.machine.resources.disk_gib,
        identity: vz::identity(),
      };
      let mut file = files::write(&temporary.path().join("identity"))?;
      serde_json::to_writer(&mut file, &identity)?;
      file.flush()?;
      file.sync_all()?;
      (self.check)()?;
      files::publish(temporary, &self.target)?;
    }
    let identity = validate(&self.target, &self.machine)?;
    let boot = vz::Linux {
      cpus: self.machine.resources.cpus as usize,
      memory: u64::from(self.machine.resources.memory_gib) << 30,
      width: 1024,
      height: 768,
      identity: identity.identity,
      boot: vz::Boot::Efi {
        variables: self.target.join("variables"),
      },
      disk: self.target.join("disk"),
      installer: self.installer,
      seed: self
        .seed
        .as_ref()
        .map(|directory| directory.path().join("seed")),
      network: Some(vz::network::Mode::Nat),
      console: None,
    };
    let mut vm = vz::create(main, &boot)?;
    vz::retain(&mut vm, Arc::new((self.runtime, self.seed)))?;
    (self.check)()?;
    owner.insert(&self.machine.id, vm)?;
    Ok(Admission {
      id: self.machine.id,
      check: self.check,
      client: self.client,
    })
  }
}

impl Admission {
  pub async fn start(self) -> anyhow::Result<()> {
    self
      .client
      .transition_checked(&self.id, vz::Action::Start, self.check)
      .await
  }
}

fn validate(target: &std::path::Path, machine: &Machine) -> anyhow::Result<Identity> {
  files::directory(target)?;
  let identity: Identity =
    serde_json::from_reader(files::read(&target.join("identity"), 65536)?.take(65537))?;
  ensure!(
    identity.id == machine.id
      && identity.version == 1
      && identity.guest == model::GuestOs::Linux
      && identity.disk_gib == machine.resources.disk_gib
      && identity.identity.len() <= 4096,
    "VZ identity or disk capacity does not match the VM record"
  );
  let mut disk = files::read(&target.join("disk"), 2048 << 30)?;
  ensure!(
    disk.metadata()?.len() == u64::from(machine.resources.disk_gib) << 30,
    "VZ disk capacity changed; preserve it for recovery"
  );
  let mut magic = [0; 4];
  disk.read_exact(&mut magic)?;
  ensure!(
    &magic != b"QFI\xfb",
    "Previous disk format requires migration"
  );
  files::read(&target.join("variables"), 16 << 20)?;
  Ok(identity)
}
