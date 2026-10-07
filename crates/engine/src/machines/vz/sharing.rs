use super::files;
use crate::machines::{Actor, Machines};
use anyhow::{ensure, Context};
use machine::vz::sharing::Directory;
use model::MachineFolder;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, io::Read, path::Path};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Grants {
  version: u32,
  id: String,
  entries: Vec<Entry>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
  folder: MachineFolder,
  device: u64,
  inode: u64,
}

fn validate(manager: &Machines, id: &str) -> anyhow::Result<()> {
  ensure!(manager.root.is_absolute(), "VZ root must be absolute");
  let machine = manager.machine(id, Actor::Person)?;
  ensure!(
    machine.runtime == Some(model::MachineRuntime::Virtualization)
      && machine.guest != model::GuestOs::Windows,
    "Shared folders require a native VZ guest"
  );
  match std::fs::symlink_metadata(manager.root.join("lima").join(id)) {
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
    Err(error) => Err(error.into()),
    Ok(_) => anyhow::bail!("Previous VM requires migration; its folders are preserved"),
  }
}

fn read(manager: &Machines, id: &str) -> anyhow::Result<Grants> {
  validate(manager, id)?;
  let empty = || Grants {
    version: 1,
    id: id.into(),
    entries: Vec::new(),
  };
  let directory = manager.root.join("sharing");
  match std::fs::symlink_metadata(&directory) {
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(empty()),
    Err(error) => return Err(error.into()),
    Ok(_) => files::directory(&directory)?,
  }
  let path = directory.join(id);
  match std::fs::symlink_metadata(&path) {
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(empty()),
    Err(error) => return Err(error.into()),
    Ok(_) => {}
  }
  let grants: Grants =
    serde_json::from_reader(files::read(&path, 128 << 10)?.take((128 << 10) + 1))?;
  ensure!(
    grants.version == 1 && grants.id == id && grants.entries.len() <= 16,
    "Invalid shared folder grants; preserve them for recovery"
  );
  let mut names = BTreeSet::new();
  for entry in &grants.entries {
    let folder = &entry.folder;
    ensure!(
      !folder.name.is_empty()
        && folder.name.len() <= 64
        && folder.name != "."
        && folder.name != ".."
        && folder
          .name
          .bytes()
          .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
        && names.insert(&folder.name)
        && Path::new(&folder.path).is_absolute()
        && folder.path.len() <= 4096
        && !folder.path.contains('\0'),
      "Invalid shared folder grant"
    );
  }
  validate(manager, id)?;
  Ok(grants)
}

pub fn folders(manager: &Machines, id: &str) -> anyhow::Result<Vec<MachineFolder>> {
  Ok(
    read(manager, id)?
      .entries
      .into_iter()
      .map(|entry| entry.folder)
      .collect(),
  )
}

fn open(entry: &Entry) -> anyhow::Result<Directory> {
  let folder = &entry.folder;
  let directory = Directory::open(&folder.name, Path::new(&folder.path), folder.read_only)?;
  ensure!(
    directory.path() == Path::new(&folder.path)
      && directory.identity()? == (entry.device, entry.inode),
    "Shared folder {} changed; remove it and select the authorized folder again",
    folder.name
  );
  Ok(directory)
}

pub(super) fn directories(manager: &Machines, id: &str) -> anyhow::Result<Vec<Directory>> {
  read(manager, id)?.entries.iter().map(open).collect()
}

fn change(
  manager: &Machines,
  id: &str,
  update: impl FnOnce(&mut Vec<Entry>) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
  validate(manager, id)?;
  let _operation = manager.lock(id)?;
  let _runtime = manager
    .guard(id, ".runtime")
    .context("Shut down and release this VM before changing shared folders")?;
  let mut grants = read(manager, id)?;
  update(&mut grants.entries)?;
  let directory = manager.root.join("sharing");
  files::parent(&directory)?;
  let mut temporary = tempfile::NamedTempFile::new_in(&directory)?;
  serde_json::to_writer(&mut temporary, &grants)?;
  ensure!(
    temporary.as_file().metadata()?.len() <= 128 << 10,
    "Shared folder grants exceed bounds"
  );
  temporary.as_file().sync_all()?;
  validate(manager, id)?;
  temporary.persist(directory.join(id))?;
  std::fs::File::open(directory)?.sync_all()?;
  Ok(())
}

pub fn add(manager: &Machines, id: &str, folder: MachineFolder) -> anyhow::Result<()> {
  change(manager, id, |entries| {
    ensure!(
      entries.len() < 16,
      "A VM supports at most 16 shared folders"
    );
    ensure!(
      !entries.iter().any(|entry| entry.folder.name == folder.name),
      "Shared folder names must be unique"
    );
    ensure!(
      folder.path.len() <= 4096,
      "Shared folder path exceeds bounds"
    );
    let directory = Directory::open(&folder.name, Path::new(&folder.path), folder.read_only)?;
    let (device, inode) = directory.identity()?;
    let path = directory
      .path()
      .to_str()
      .context("Shared folder path must be UTF-8")?
      .into();
    entries.push(Entry {
      folder: MachineFolder { path, ..folder },
      device,
      inode,
    });
    Ok(())
  })
}

pub fn remove(manager: &Machines, id: &str, name: &str) -> anyhow::Result<()> {
  change(manager, id, |entries| {
    let position = entries
      .iter()
      .position(|entry| entry.folder.name == name)
      .context("Shared folder grant is missing")?;
    entries.remove(position);
    Ok(())
  })
}

pub fn set_read_only(
  manager: &Machines,
  id: &str,
  name: &str,
  read_only: bool,
) -> anyhow::Result<()> {
  change(manager, id, |entries| {
    let entry = entries
      .iter_mut()
      .find(|entry| entry.folder.name == name)
      .context("Shared folder grant is missing")?;
    open(entry)?;
    entry.folder.read_only = read_only;
    Ok(())
  })
}
