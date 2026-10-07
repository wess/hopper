pub mod media;

use super::{files, intent, Service};
use crate::machines::Actor;
use anyhow::{ensure, Context};
use machine::vz::{queue::Check, restore};
use model::{GuestOs, MachineRuntime};
use std::{
  path::Path,
  sync::Arc,
  time::{Duration, Instant},
};

pub struct Restore {
  directory: Arc<tempfile::TempDir>,
  image: restore::Image,
  check: Check,
}

impl Restore {
  pub fn image(&self) -> &restore::Image {
    &self.image
  }

  pub fn path(&self) -> std::path::PathBuf {
    self.directory.path().join("restore.ipsw")
  }

  pub fn authorized(&self) -> anyhow::Result<()> {
    (self.check)()
  }
}

impl Service {
  pub async fn inspect_mac(&self, id: &str, actor: Actor) -> anyhow::Result<Restore> {
    let (machine, original) = self.scope(id, actor)?;
    ensure!(
      machine.guest == GuestOs::Macos
        && machine.profile == "macos"
        && machine.runtime == Some(MachineRuntime::Virtualization),
      "Restore inspection requires native macOS"
    );
    crate::machines::config::validate_resources(&machine)?;
    let manager = self.manager.clone();
    let identity = id.to_owned();
    let check: Check = Arc::new(move || {
      original()?;
      ensure!(
        manager.machine(&identity, actor)?.profile == "macos",
        "macOS restore profile changed"
      );
      Ok(())
    });
    let control = intent::read(&self.manager, id)?;
    let check = intent::checked(self.manager.clone(), id.into(), control, check);
    let path = Path::new(
      machine
        .installer
        .as_deref()
        .context("Acquire macOS restore media before inspection")?,
    )
    .to_owned();
    let parent = self.manager.root.join("vz");
    let scope = check.clone();
    let directory = tokio::task::spawn_blocking(move || {
      scope()?;
      files::parent(&parent)?;
      media::stage(&path, &parent, &scope)
    })
    .await??;
    check()?;
    let directory = Arc::new(directory);
    let discovery = restore::local_owned(
      &directory.path().join("restore.ipsw"),
      Arc::new((directory.clone(), check.clone())),
    )?;
    let deadline = Instant::now() + Duration::from_secs(120);
    let image = loop {
      check()?;
      if let Some(result) = restore::poll(&discovery)? {
        break result?;
      }
      ensure!(
        Instant::now() < deadline,
        "macOS restore inspection timed out"
      );
      tokio::time::sleep(Duration::from_millis(25)).await;
    };
    check()?;
    ensure!(
      machine.resources.cpus as usize >= image.minimum_cpus
        && u64::from(machine.resources.memory_gib) << 30 >= image.minimum_memory,
      "macOS resources are below this restore image's requirements"
    );
    Ok(Restore {
      directory,
      image,
      check,
    })
  }
}
