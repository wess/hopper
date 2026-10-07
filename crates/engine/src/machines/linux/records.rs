use crate::machines::{native::assets, Actor, Machines};
use anyhow::{ensure, Context};
use model::{CreateMachine, GuestOs, Machine, MachineRuntime, MachineStatus};
use std::path::Path;

pub fn create(manager: &Machines, request: CreateMachine) -> anyhow::Result<Machine> {
  ensure!(
    request.profile == "ubuntu",
    "Choose a native Ubuntu VM profile"
  );
  ensure!(
    !request.name.trim().is_empty() && request.name.len() <= 100,
    "Choose a VM name of 1–100 characters"
  );
  let machine = Machine {
    id: model::new_uuid(),
    name: request.name.trim().into(),
    guest: GuestOs::Linux,
    profile: request.profile,
    resources: request.resources,
    runtime: Some(MachineRuntime::Virtualization),
    agent_access: request.agent_access,
    agent_generation: 0,
    installer: request.installer,
  };
  crate::machines::config::validate_resources(&machine)?;
  if let Some(installer) = &machine.installer {
    ensure!(
      Path::new(installer).is_absolute(),
      "Linux installer must be absolute"
    );
    assets::regular(Path::new(installer), 16 << 30)?;
  }
  let _operation = manager.lock(&machine.id)?;
  let record = manager.record(&machine.id)?;
  ensure!(!record.try_exists()?, "VM identity already exists");
  store::json::write(&record, &machine)?;
  Ok(machine)
}

pub(crate) fn status(
  manager: &Machines,
  machine: Machine,
  actor: Actor,
) -> anyhow::Result<MachineStatus> {
  manager.machine(&machine.id, actor)?;
  match std::fs::symlink_metadata(manager.root.join("lima").join(&machine.id)) {
    Ok(_) => {
      return Ok(MachineStatus {
        machine,
        state: "Migration required".into(),
        busy: false,
        progress: Some("The previous VM disk is preserved for migration".into()),
      })
    }
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
    Err(error) => return Err(error.into()),
  }
  let operation = match manager.lock(&machine.id) {
    Ok(lease) => Some(lease),
    Err(error) if busy(&error) => None,
    Err(error) => return Err(error),
  };
  let (state, busy, progress) = if operation.is_none() {
    (
      "Unavailable",
      true,
      Some("Another operation is in progress."),
    )
  } else {
    match manager.guard(&machine.id, ".runtime") {
      Ok(_lease) => {
        let prepared = match std::fs::symlink_metadata(manager.root.join("vz").join(&machine.id)) {
          Ok(_) => true,
          Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
          Err(error) => return Err(error.into()),
        };
        let phase = if prepared {
          super::progress::read(&manager.root.join("vz").join(&machine.id))?
        } else {
          None
        };
        (
          match phase {
            Some(super::progress::Phase::Deployed) => "Deployment finished",
            Some(_) => "Installation recovery required",
            None if prepared => "Ready to start",
            None => "Not created",
          },
          false,
          phase.map(|phase| phase.message()),
        )
      }
      Err(error) if busy(&error) => (
        "Unavailable",
        true,
        Some("This VM is owned by another runtime."),
      ),
      Err(error) => return Err(error).context("Inspect native VM runtime ownership"),
    }
  };
  Ok(MachineStatus {
    machine,
    state: state.into(),
    busy,
    progress: progress.map(Into::into),
  })
}

fn busy(error: &anyhow::Error) -> bool {
  error
    .chain()
    .filter_map(|error| error.downcast_ref::<std::io::Error>())
    .any(|error| error.kind() == std::io::ErrorKind::WouldBlock)
}
