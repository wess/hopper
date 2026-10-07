//! Per-VM boot paths. Deployment accepts only an unwritten private sparse target.

use super::assets::{self, Assets};
use crate::machines::{Actor, Machines};
use anyhow::{ensure, Context};
use model::native::Boot;
use sha2::{Digest, Sha256};
use std::{
  io::Read,
  path::{Path, PathBuf},
};

#[derive(Clone, Copy)]
pub enum Stage {
  Deployment,
  System,
}

pub struct Paths {
  pub root: PathBuf,
  pub variables: PathBuf,
  pub disk: PathBuf,
  pub setup: PathBuf,
}

pub fn paths(root: &Path, id: &str) -> anyhow::Result<Paths> {
  ensure!(root.is_absolute(), "Native VM root must be absolute");
  crate::machines::validate_id(id)?;
  ensure!(
    uuid::Uuid::parse_str(id)?.to_string() == id,
    "Native VM identity must be canonical"
  );
  let root = root.join("native").join(id);
  Ok(Paths {
    variables: root.join("variables"),
    disk: root.join("disk"),
    setup: root.join("setup/deployment.iso"),
    root,
  })
}

pub fn initialize(manager: &Machines, id: &str) -> anyhow::Result<Paths> {
  let machine = manager.machine(id, Actor::Person)?;
  validate(&machine)?;
  let paths = paths(&manager.root, id)?;
  for folder in [
    manager.root.join("native"),
    paths.root.clone(),
    paths.root.join("setup"),
  ] {
    if !folder.exists() {
      let mut builder = std::fs::DirBuilder::new();
      #[cfg(unix)]
      {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
      }
      builder.create(&folder)?;
    }
    private_directory(&folder)?;
  }
  Ok(paths)
}

pub fn prepare(
  manager: &Machines,
  id: &str,
  assets: &Assets,
  stage: Stage,
) -> anyhow::Result<Boot> {
  let machine = manager.machine(id, Actor::Person)?;
  validate(&machine)?;
  let folder = assets
    .firmware
    .parent()
    .context("Native firmware directory is missing")?;
  let assets = assets::verify(&assets.worker, folder)?;
  let paths = paths(&manager.root, id)?;
  private_directory(&manager.root.join("native"))?;
  private_directory(&paths.root)?;
  let disk = target(&paths, &machine, matches!(stage, Stage::Deployment))?;
  let (boot_media, installer) = match stage {
    Stage::Deployment => {
      private_directory(&paths.root.join("setup"))?;
      verify_setup(&paths.setup, id)?;
      let installer = machine
        .installer
        .as_deref()
        .context("Prepare the Windows installer before native deployment")?;
      ensure!(
        Path::new(installer).is_absolute(),
        "Installer path must be absolute"
      );
      assets::regular(Path::new(installer), 12 * 1024 * 1024 * 1024)?;
      (Some(text(&paths.setup)?), Some(installer.to_owned()))
    }
    Stage::System => {
      #[cfg(unix)]
      {
        use std::os::unix::fs::MetadataExt;
        ensure!(
          disk.metadata()?.blocks() > 0,
          "System disk has not received installation data"
        );
      }
      let mut disk = &*disk;
      let mut magic = [0; 4];
      disk.read_exact(&mut magic)?;
      ensure!(
        magic != *b"QFI\xfb",
        "Preserve the previous disk format for migration"
      );
      (None, None)
    }
  };
  let digest = format!(
    "{:x}",
    Sha256::digest(uuid::Uuid::parse_str(id)?.as_bytes())
  );
  Ok(Boot {
    firmware: text(&assets.firmware)?,
    variables: text(&assets.variables)?,
    store: text(&paths.variables)?,
    boot_media,
    installer,
    disk: Some(text(&paths.disk)?),
    disk_id: format!("hpr{}", &digest[..17]),
    memory: u64::from(machine.resources.memory_gib) * 1024 * 1024 * 1024,
    cpus: machine.resources.cpus,
    timeout_ms: None,
  })
}

pub(crate) fn validate(machine: &model::Machine) -> anyhow::Result<()> {
  ensure!(
    machine.guest == model::GuestOs::Windows,
    "Native Windows startup needs a Windows VM"
  );
  ensure!(
    (1..=2).contains(&machine.resources.cpus),
    "Native Windows currently supports one or two CPUs"
  );
  ensure!(
    (1..=64).contains(&machine.resources.memory_gib),
    "Native Windows memory must be 1–64 GiB"
  );
  ensure!(
    (64..=2048).contains(&machine.resources.disk_gib),
    "Native Windows disk must be 64–2048 GiB"
  );
  Ok(())
}

fn private_directory(path: &Path) -> anyhow::Result<()> {
  let info = std::fs::symlink_metadata(path)?;
  ensure!(info.is_dir(), "Native VM directory must not be a symlink");
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    ensure!(
      info.permissions().mode() & 0o077 == 0,
      "Native VM directory must be private"
    );
  }
  Ok(())
}

fn private_file(file: &std::fs::File) -> anyhow::Result<()> {
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    ensure!(
      file.metadata()?.permissions().mode() & 0o077 == 0,
      "Native VM file must be private"
    );
  }
  Ok(())
}

fn text(path: &Path) -> anyhow::Result<String> {
  Ok(
    path
      .to_str()
      .context("Native path is not UTF-8")?
      .to_owned(),
  )
}

pub(super) fn verify_setup(path: &Path, id: &str) -> anyhow::Result<()> {
  #[derive(serde::Deserialize)]
  #[serde(rename_all = "camelCase")]
  struct Manifest {
    vm_id: String,
    size: u64,
    sha256: String,
    contains_guest_credentials: bool,
    detach_before_first_boot: bool,
  }
  let metadata = assets::regular(&path.with_extension("json"), 1024 * 1024)?;
  private_file(&metadata)?;
  let manifest: Manifest = serde_json::from_reader(metadata.take(1024 * 1024 + 1))?;
  ensure!(
    manifest.vm_id == id
      && manifest.contains_guest_credentials
      && manifest.detach_before_first_boot,
    "Setup media does not belong to this VM or has invalid first-boot policy"
  );
  ensure!(
    (1..=2 * 1024 * 1024 * 1024).contains(&manifest.size),
    "Invalid setup image size"
  );
  ensure!(
    manifest.sha256.len() == 64 && manifest.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()),
    "Invalid setup image checksum"
  );
  let file = assets::regular(path, manifest.size)?;
  private_file(&file)?;
  ensure!(
    file.metadata()?.len() == manifest.size,
    "Setup image size mismatch"
  );
  let mut file = file.take(manifest.size + 1);
  let mut hash = Sha256::new();
  let mut total = 0u64;
  let mut buffer = [0; 64 * 1024];
  loop {
    let read = file.read(&mut buffer)?;
    if read == 0 {
      break;
    }
    total += read as u64;
    hash.update(&buffer[..read]);
  }
  ensure!(
    total == manifest.size
      && format!("{:x}", hash.finalize()).eq_ignore_ascii_case(&manifest.sha256),
    "Setup image checksum mismatch"
  );
  Ok(())
}

pub(super) fn target(
  paths: &Paths,
  machine: &model::Machine,
  unwritten: bool,
) -> anyhow::Result<store::lock::Lease> {
  let disk = assets::regular(&paths.disk, 2048 * 1024 * 1024 * 1024)?;
  private_file(&disk)?;
  let disk = store::lock::exclusive(disk).context("Native disk is already in use")?;
  ensure!(
    disk.metadata()?.len() == u64::from(machine.resources.disk_gib) * 1024 * 1024 * 1024,
    "Native disk capacity does not match the VM record"
  );
  if unwritten {
    #[cfg(unix)]
    {
      use std::os::unix::fs::MetadataExt;
      ensure!(
        disk.metadata()?.blocks() == 0,
        "Deployment target already contains allocated data; preserve it for recovery"
      );
    }
    #[cfg(not(unix))]
    anyhow::bail!("Native deployment requires a Unix host");
  }
  Ok(disk)
}
