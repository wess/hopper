pub mod acquire;
mod admit;
pub mod download;
mod launch;
pub use launch::{Admission, Launch, Phase};
pub mod deployment;
pub mod media;
pub mod platform;

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
  manager: crate::machines::Machines,
  machine: model::Machine,
}

pub struct System {
  platform: platform::Prepared,
}

impl System {
  pub fn admit(
    self,
    main: machine::vz::MainThreadMarker,
    owner: &mut super::Owner,
  ) -> anyhow::Result<()> {
    self.platform.admit_system(main, owner)
  }
}

pub struct Prepared {
  platform: platform::Prepared,
  media: Arc<tempfile::TempDir>,
}

impl Prepared {
  pub fn install(
    self,
    main: machine::vz::MainThreadMarker,
    owner: &mut super::Owner,
  ) -> anyhow::Result<machine::vz::queue::Installation> {
    let id = self.platform.machine.id.clone();
    let check = self.platform.check.clone();
    let path = self.media.path().join("restore.ipsw");
    let directory = self.platform.target.clone();
    ensure!(
      !self.platform.installed,
      "macOS is installed; choose system boot"
    );
    self.admit(main, owner)?;
    let attempt = match (|| {
      check()?;
      deployment::begin(&directory, &id)
    })() {
      Ok(attempt) => attempt,
      Err(error) => {
        owner.retire(&id)?;
        return Err(error);
      }
    };
    let authorized = check.clone();
    let commit: Check = Arc::new(move || {
      authorized()?;
      deployment::installed(&attempt)?;
      authorized()?;
      Ok(())
    });
    owner.install_committed(&id, path, check, commit)
  }

  pub fn admit(
    self,
    main: machine::vz::MainThreadMarker,
    owner: &mut super::Owner,
  ) -> anyhow::Result<()> {
    self.platform.admit(main, owner, self.media)
  }
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

  pub async fn prepare(self) -> anyhow::Result<Prepared> {
    self.authorized()?;
    tokio::task::spawn_blocking(move || {
      let platform = platform::prepare(&self.manager, self.machine, self.image, self.check)?;
      Ok(Prepared {
        platform,
        media: self.directory,
      })
    })
    .await?
  }
}

impl Service {
  pub async fn prepare_mac_system(&self, id: &str, actor: Actor) -> anyhow::Result<System> {
    let (machine, check) = self.scope(id, actor)?;
    ensure!(
      machine.guest == GuestOs::Macos
        && machine.profile == "macos"
        && machine.runtime == Some(MachineRuntime::Virtualization),
      "System boot requires native macOS"
    );
    let control = intent::read(&self.manager, id)?;
    let check = intent::checked(self.manager.clone(), id.into(), control, check);
    let manager = self.manager.clone();
    tokio::task::spawn_blocking(move || {
      Ok(System {
        platform: platform::system(&manager, machine, check)?,
      })
    })
    .await?
  }

  pub async fn inspect_mac(&self, id: &str, actor: Actor) -> anyhow::Result<Restore> {
    let (machine, check) = self.mac_scope(id, actor)?;
    self.inspect_restore(machine, check).await
  }

  fn mac_scope(&self, id: &str, actor: Actor) -> anyhow::Result<(model::Machine, Check)> {
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
    Ok((machine, check))
  }

  async fn inspect_restore(
    &self,
    machine: model::Machine,
    check: Check,
  ) -> anyhow::Result<Restore> {
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
      manager: self.manager.clone(),
      machine,
    })
  }
}
