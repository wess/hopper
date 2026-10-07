use super::files;
use crate::machines::{Actor, Machines};
use anyhow::{ensure, Context};
use serde::{Deserialize, Serialize};
use std::io::Read;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Audio {
  version: u32,
  id: String,
  speakers: bool,
}

fn validate(manager: &Machines, id: &str, actor: Actor) -> anyhow::Result<()> {
  ensure!(manager.root.is_absolute(), "VZ root must be absolute");
  let machine = manager.machine(id, actor)?;
  ensure!(
    machine.runtime == Some(model::MachineRuntime::Virtualization)
      && machine.guest != model::GuestOs::Windows,
    "Audio settings require a native VZ guest"
  );
  match std::fs::symlink_metadata(manager.root.join("lima").join(id)) {
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
    Err(error) => Err(error.into()),
    Ok(_) => anyhow::bail!("Previous VM requires migration; its audio settings are preserved"),
  }
}

pub fn speakers(manager: &Machines, id: &str, actor: Actor) -> anyhow::Result<bool> {
  validate(manager, id, actor)?;
  let directory = manager.root.join("audio");
  match std::fs::symlink_metadata(&directory) {
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(true),
    Err(error) => return Err(error.into()),
    Ok(_) => files::directory(&directory)?,
  }
  let path = directory.join(id);
  match std::fs::symlink_metadata(&path) {
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(true),
    Err(error) => return Err(error.into()),
    Ok(_) => {}
  }
  let audio: Audio = serde_json::from_reader(files::read(&path, 1024)?.take(1025))?;
  ensure!(
    audio.version == 1 && audio.id == id,
    "Invalid VM audio settings; preserve them for recovery"
  );
  validate(manager, id, actor)?;
  Ok(audio.speakers)
}

pub fn set_speakers(manager: &Machines, id: &str, speakers: bool) -> anyhow::Result<()> {
  validate(manager, id, Actor::Person)?;
  let _operation = manager.lock(id)?;
  let _runtime = manager
    .guard(id, ".runtime")
    .context("Shut down and release this VM before changing its speakers")?;
  // preserve malformed settings instead of silently enabling audio.
  self::speakers(manager, id, Actor::Person)?;
  let directory = manager.root.join("audio");
  files::parent(&directory)?;
  let mut temporary = tempfile::NamedTempFile::new_in(&directory)?;
  serde_json::to_writer(
    &mut temporary,
    &Audio {
      version: 1,
      id: id.into(),
      speakers,
    },
  )?;
  temporary.as_file().sync_all()?;
  validate(manager, id, Actor::Person)?;
  temporary.persist(directory.join(id))?;
  std::fs::File::open(directory)?.sync_all()?;
  Ok(())
}
