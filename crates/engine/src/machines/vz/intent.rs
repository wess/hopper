use super::files;
use crate::machines::{Actor, Machines};
use anyhow::ensure;
use machine::vz::queue::Check;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Intent {
  version: u32,
  stop: String,
}

pub(super) fn read(manager: &Machines, id: &str) -> anyhow::Result<Option<String>> {
  ensure!(manager.root.is_absolute(), "VZ root must be absolute");
  crate::machines::validate_id(id)?;
  let directory = manager.root.join("intent");
  match std::fs::symlink_metadata(&directory) {
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
    Err(error) => return Err(error.into()),
    Ok(_) => files::directory(&directory)?,
  }
  let path = directory.join(id);
  match std::fs::symlink_metadata(&path) {
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
    Err(error) => return Err(error.into()),
    Ok(_) => {}
  }
  let intent: Intent = serde_json::from_reader(files::read(&path, 1024)?)?;
  ensure!(
    intent.version == 1 && uuid::Uuid::parse_str(&intent.stop)?.to_string() == intent.stop,
    "Invalid VM stop intent"
  );
  Ok(Some(intent.stop))
}

pub(super) fn cancel(
  manager: &Machines,
  original: &model::Machine,
  actor: Actor,
) -> anyhow::Result<()> {
  ensure!(manager.root.is_absolute(), "VZ root must be absolute");
  let id = &original.id;
  same(manager, original, actor)?;
  let _lease = manager.guard(id, ".intent")?;
  same(manager, original, actor)?;
  let directory = manager.root.join("intent");
  files::parent(&directory)?;
  let mut temporary = tempfile::NamedTempFile::new_in(&directory)?;
  serde_json::to_writer(
    &mut temporary,
    &Intent {
      version: 1,
      stop: uuid::Uuid::new_v4().to_string(),
    },
  )?;
  temporary.as_file().sync_all()?;
  same(manager, original, actor)?;
  temporary.persist(directory.join(id))?;
  std::fs::File::open(directory)?.sync_all()?;
  same(manager, original, actor)
}

pub(super) fn checked(
  manager: Machines,
  id: String,
  expected: Option<String>,
  check: Check,
) -> Check {
  Arc::new(move || {
    check()?;
    ensure!(
      read(&manager, &id)? == expected,
      "VM startup cancelled by an explicit stop request"
    );
    check()
  })
}

fn same(manager: &Machines, original: &model::Machine, actor: Actor) -> anyhow::Result<()> {
  let current = manager.machine(&original.id, actor)?;
  ensure!(
    current.guest == original.guest
      && current.runtime == original.runtime
      && (actor != Actor::Agent || current.agent_generation == original.agent_generation),
    "VM stop policy changed"
  );
  Ok(())
}
