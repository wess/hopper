//! Durable setup progress. Deployment completion never implies a usable desktop.

use super::{assets, config};
use crate::machines::{Actor, Machines};
use anyhow::ensure;
use model::native::{Installation, SetupStatus};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
  version: u32,
  vm_id: String,
  installation: Installation,
}

pub fn read(manager: &Machines, id: &str, actor: Actor) -> anyhow::Result<Option<Installation>> {
  manager.machine(id, actor)?;
  let path = config::paths(&manager.root, id)?.root.join("installation");
  match std::fs::symlink_metadata(&path) {
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
    Err(error) => return Err(error.into()),
    Ok(_) => {}
  }
  let file = assets::regular(&path, 16 * 1024)?;
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    ensure!(
      file.metadata()?.permissions().mode() & 0o077 == 0,
      "Installation record must be private"
    );
  }
  let record: Record = serde_json::from_reader(file.take(16 * 1024 + 1))?;
  ensure!(
    record.version == 1 && record.vm_id == id,
    "Installation record identity mismatch"
  );
  Ok(Some(record.installation))
}

/// The caller holds the VM operation lease through publication.
pub(crate) fn save(manager: &Machines, id: &str, installation: Installation) -> anyhow::Result<()> {
  read(manager, id, Actor::Person)?;
  let folder = config::initialize(manager, id)?.root;
  let mut temporary = tempfile::NamedTempFile::new_in(&folder)?;
  let record = Record {
    version: 1,
    vm_id: id.into(),
    installation,
  };
  temporary.write_all(&serde_json::to_vec(&record)?)?;
  temporary.as_file().sync_all()?;
  temporary.persist(folder.join("installation"))?;
  std::fs::File::open(folder)?.sync_all()?;
  Ok(())
}

pub fn observed(previous: &Installation, next: SetupStatus) -> anyhow::Result<Installation> {
  let old = match previous {
    Installation::Preparing {} => None,
    Installation::Setup { status } => Some(status),
    Installation::Deployed {} if matches!(&next, SetupStatus::Deployed {}) => {
      return Ok(Installation::Deployed {});
    }
    _ => anyhow::bail!("Installation is not accepting setup progress"),
  };
  if let Some(old) = old {
    match (old, &next) {
      (SetupStatus::Failed { .. } | SetupStatus::Invalid {} | SetupStatus::Deployed {}, _) => {
        ensure!(old == &next, "Setup advanced after a terminal failure");
      }
      (
        SetupStatus::Active { phase: before },
        SetupStatus::Active { phase: after } | SetupStatus::Failed { phase: after },
      ) => {
        ensure!(*after as u8 >= *before as u8, "Setup phase moved backwards");
      }
      (SetupStatus::Active { .. }, SetupStatus::Waiting {}) => {
        anyhow::bail!("Setup returned to waiting")
      }
      _ => {}
    }
  }
  Ok(if matches!(&next, SetupStatus::Deployed {}) {
    Installation::Deployed {}
  } else {
    Installation::Setup { status: next }
  })
}

pub fn boot_stage(installation: Option<&Installation>) -> anyhow::Result<config::Stage> {
  match installation {
    None
    | Some(
      Installation::Preparing {}
      | Installation::Setup {
        status: SetupStatus::Waiting {},
      }
      | Installation::Interrupted {
        status: SetupStatus::Waiting {},
      },
    ) => Ok(config::Stage::Deployment),
    Some(
      Installation::Deployed {}
      | Installation::Booting {}
      | Installation::SystemStarted {}
      | Installation::HandoffFailed {},
    ) => Ok(config::Stage::System),
    _ => anyhow::bail!(
      "Windows setup needs recovery before restarting; existing disk data is retained"
    ),
  }
}

pub(crate) fn interrupted(manager: &Machines, id: &str) -> anyhow::Result<()> {
  if let Some(Installation::Setup { status }) = read(manager, id, Actor::Person)? {
    save(manager, id, Installation::Interrupted { status })?;
  }
  Ok(())
}
