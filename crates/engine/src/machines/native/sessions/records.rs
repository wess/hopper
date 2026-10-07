use super::{
  super::{assets, config, installation, State},
  Sessions,
};
use crate::machines::Actor;
use anyhow::{ensure, Context};
use model::{
  native::{Installation, SetupPhase, SetupStatus},
  CreateMachine, GuestOs, Machine, MachineStatus,
};
use std::path::Path;

impl Sessions {
  pub fn create_windows(&self, request: CreateMachine) -> anyhow::Result<Machine> {
    ensure!(
      cfg!(all(target_os = "macos", target_arch = "aarch64")),
      "Desktop VMs require an Apple silicon Mac"
    );
    ensure!(
      !request.name.trim().is_empty() && request.name.len() <= 100,
      "Choose a VM name of 1–100 characters"
    );
    let profile = self
      .inner
      .manager
      .profiles()
      .into_iter()
      .find(|profile| profile.id == request.profile && profile.guest == GuestOs::Windows)
      .context("Choose a Windows VM profile")?;
    let machine = Machine {
      id: model::new_uuid(),
      name: request.name.trim().into(),
      guest: GuestOs::Windows,
      profile: profile.id,
      resources: request.resources,
      installer: request.installer,
      agent_access: request.agent_access,
      agent_generation: 0,
      runtime: Some(model::MachineRuntime::Hypervisor),
    };
    config::validate(&machine)?;
    config::paths(&self.inner.manager.root, &machine.id)?;
    if let Some(installer) = &machine.installer {
      ensure!(
        Path::new(installer).is_absolute(),
        "Installer path must be absolute"
      );
      assets::regular(Path::new(installer), 12 * 1024 * 1024 * 1024)?;
    }
    let _operation = self.inner.manager.lock(&machine.id)?;
    let path = self.inner.manager.record(&machine.id)?;
    ensure!(!path.exists(), "VM identity already exists");
    store::json::write(&path, &machine)?;
    Ok(machine)
  }

  /// Native records and hardware state do not require the previous VM helper.
  pub async fn list_windows(&self, actor: Actor) -> anyhow::Result<Vec<MachineStatus>> {
    let manager = &self.inner.manager;
    let mut rows = Vec::new();
    for machine in manager.records(actor)? {
      if machine.guest != GuestOs::Windows {
        continue;
      }
      manager.machine(&machine.id, actor)?;
      if machine.runtime != Some(model::MachineRuntime::Hypervisor)
        || manager.root.join("lima").join(&machine.id).try_exists()?
      {
        rows.push(MachineStatus {
          machine,
          state: "Migration required".into(),
          busy: false,
          progress: Some("The previous VM disk is preserved for migration".into()),
        });
        continue;
      }
      let slot = self.slot(&machine.id).await?;
      let hardware = slot.state.borrow().clone();
      let current = installation::read(manager, &machine.id, actor)?;
      let operation = match manager.lock(&machine.id) {
        Ok(lease) => Some(lease),
        Err(error)
          if error
            .chain()
            .filter_map(|error| error.downcast_ref::<std::io::Error>())
            .any(|error| error.kind() == std::io::ErrorKind::WouldBlock) =>
        {
          None
        }
        Err(error) => return Err(error),
      };
      let busy = operation.is_none();
      let foreign = !busy
        && matches!(hardware, State::Stopped(_) | State::Failed(_))
        && match manager.guard(&machine.id, ".runtime") {
          Ok(_) => false,
          Err(error)
            if error
              .chain()
              .filter_map(|error| error.downcast_ref::<std::io::Error>())
              .any(|error| error.kind() == std::io::ErrorKind::WouldBlock) =>
          {
            true
          }
          Err(error) => return Err(error),
        };
      let state = if foreign {
        "Running elsewhere"
      } else {
        match hardware {
          State::Running => "Running",
          State::Paused => "Paused",
          State::Failed(_) => "Failed",
          State::Stopped(_) => match &current {
            None => "Not created",
            Some(Installation::Interrupted { status })
              if !matches!(status, model::native::SetupStatus::Waiting {}) =>
            {
              "Recovery required"
            }
            Some(Installation::HandoffFailed {}) => "Stopped",
            _ => "Stopped",
          },
        }
      };
      let progress = if foreign {
        Some("This VM is owned by another Hopper process".into())
      } else if state == "Stopped" && matches!(current, Some(Installation::SystemStarted {})) {
        None
      } else {
        current.as_ref().map(progress)
      };
      rows.push(MachineStatus {
        machine,
        state: state.into(),
        busy: busy || foreign,
        progress,
      });
    }
    rows.sort_by_key(|row| row.machine.name.to_lowercase());
    Ok(rows)
  }
}

fn progress(installation: &Installation) -> String {
  match installation {
    Installation::Preparing {} => "Preparing Windows installation media".into(),
    Installation::Setup { status } => setup(*status),
    Installation::Interrupted { status } => format!("Setup interrupted. {}", setup(*status)),
    Installation::Deployed {} => "Windows installed; waiting to start the system".into(),
    Installation::Booting {} => "Starting Windows from its system disk".into(),
    Installation::SystemStarted {} => "Waiting for the Windows desktop".into(),
    Installation::HandoffFailed {} => {
      "System startup failed; the installed disk is retained".into()
    }
  }
}

fn setup(status: SetupStatus) -> String {
  match status {
    SetupStatus::Waiting {} => "Waiting for Windows setup".into(),
    SetupStatus::Deployed {} => "Windows installed".into(),
    SetupStatus::Invalid {} => {
      "Windows setup returned invalid progress; the disk is retained".into()
    }
    SetupStatus::Active { phase } => phase_name(phase).into(),
    SetupStatus::Failed { phase } => {
      format!("Setup failed: {}. The disk is retained", phase_name(phase))
    }
  }
}

fn phase_name(phase: SetupPhase) -> &'static str {
  match phase {
    SetupPhase::Files => "Loading installation files",
    SetupPhase::Pe => "Starting Windows setup",
    SetupPhase::Drivers => "Loading hardware drivers",
    SetupPhase::Media => "Finding the Windows installer",
    SetupPhase::Image => "Verifying the Windows image",
    SetupPhase::Letters => "Preparing installation drives",
    SetupPhase::Partition => "Partitioning the system disk",
    SetupPhase::Apply => "Installing Windows",
    SetupPhase::OfflineDrivers => "Installing hardware drivers",
    SetupPhase::Provision => "Preparing your Windows accounts",
    SetupPhase::Recovery => "Configuring Windows recovery",
    SetupPhase::Boot => "Preparing Windows to start",
  }
}
