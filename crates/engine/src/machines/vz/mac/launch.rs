use super::{deployment, Prepared, System};
use crate::machines::{
  vz::{intent, Service},
  Actor,
};
use anyhow::ensure;
use machine::vz::{
  queue::{Check, Client, Installation, Owner},
  Action, MainThreadMarker,
};
use tokio::sync::watch;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
  Inspecting,
  Preparing,
  Installing(u8),
  Starting,
}

impl Phase {
  pub fn message(self) -> String {
    match self {
      Self::Inspecting => "Checking macOS restore image…".into(),
      Self::Preparing => "Preparing macOS hardware…".into(),
      Self::Installing(percent) => format!("Installing macOS… {percent}%"),
      Self::Starting => "Starting macOS…".into(),
    }
  }
}

enum Stage {
  Install(Prepared),
  System(System),
}

pub struct Launch {
  stage: Stage,
  id: String,
  check: Check,
  client: Client,
}

pub struct Admission {
  installation: Option<Installation>,
  id: String,
  check: Check,
  client: Client,
}

impl Launch {
  pub fn authorized(&self) -> anyhow::Result<()> {
    (self.check)()
  }

  pub fn admit(self, main: MainThreadMarker, owner: &mut Owner) -> anyhow::Result<Admission> {
    self.authorized()?;
    let installation = match self.stage {
      Stage::Install(prepared) => Some(prepared.install(main, owner)?),
      Stage::System(system) => {
        system.admit(main, owner)?;
        None
      }
    };
    Ok(Admission {
      installation,
      id: self.id,
      check: self.check,
      client: self.client,
    })
  }
}

impl Admission {
  pub async fn start(self, progress: watch::Sender<Phase>) -> anyhow::Result<()> {
    (self.check)()?;
    if let Some(installation) = self.installation {
      progress.send_replace(Phase::Installing(0));
      let mut updates = installation.progress();
      let completion = installation.wait();
      tokio::pin!(completion);
      loop {
        tokio::select! {
          biased;
          result = &mut completion => { result?; break; }
          changed = updates.changed() => {
            if changed.is_ok() {
              let fraction = *updates.borrow_and_update();
              progress.send_replace(Phase::Installing((fraction.clamp(0.0, 1.0) * 100.0) as u8));
            }
          }
        }
      }
    }
    (self.check)()?;
    progress.send_replace(Phase::Starting);
    self
      .client
      .transition_checked(&self.id, Action::Start, self.check)
      .await
  }
}

impl Service {
  pub async fn prepare_mac(
    &self,
    id: &str,
    actor: Actor,
    progress: watch::Sender<Phase>,
  ) -> anyhow::Result<Launch> {
    let (machine, check) = self.mac_scope(id, actor)?;
    progress.send_replace(Phase::Inspecting);
    let directory = self.manager.root.join("vz").join(id);
    let phase = match std::fs::symlink_metadata(&directory) {
      Ok(_) => deployment::read(&directory, id)?,
      Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
      Err(error) => return Err(error.into()),
    };
    ensure!(
      phase != Some(deployment::Phase::Installing),
      "macOS installation requires recovery; its disk is preserved"
    );
    let stage = if phase == Some(deployment::Phase::Installed) {
      progress.send_replace(Phase::Preparing);
      let manager = self.manager.clone();
      let original = check.clone();
      Stage::System(
        tokio::task::spawn_blocking(move || {
          Ok::<_, anyhow::Error>(System {
            platform: super::platform::system(&manager, machine, original)?,
          })
        })
        .await??,
      )
    } else {
      let restore = self.inspect_restore(machine, check.clone()).await?;
      progress.send_replace(Phase::Preparing);
      Stage::Install(restore.prepare().await?)
    };
    check()?;
    Ok(Launch {
      stage,
      id: id.into(),
      check,
      client: self.client.clone(),
    })
  }

  pub fn cancel_mac(&self, id: &str, actor: Actor) -> anyhow::Result<()> {
    let original = self.manager.machine(id, actor)?;
    ensure!(
      original.guest == model::GuestOs::Macos
        && original.profile == "macos"
        && original.runtime == Some(model::MachineRuntime::Virtualization),
      "Setup cancellation requires native macOS"
    );
    ensure!(
      !self.manager.root.join("lima").join(id).try_exists()?,
      "Previous VM requires migration; its disk is preserved"
    );
    intent::cancel(&self.manager, &original, actor)
  }
}
