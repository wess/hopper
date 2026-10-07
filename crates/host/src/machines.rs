use crate::{Host, MachineActor};
use model::{CreateMachine, Machine, MachineStatus};

impl Host {
  #[cfg(unix)]
  pub async fn native_machine_lifecycle(
    &self,
    id: &str,
    action: ::engine::machines::native::sessions::remote::Lifecycle,
  ) -> anyhow::Result<()> {
    ::engine::machines::native::sessions::remote::lifecycle(&self.machines(), id, action).await
  }
  #[cfg(unix)]
  pub fn serve_machine_agents(&self) -> anyhow::Result<()> {
    let mut server = self
      .machine_agents
      .lock()
      .map_err(|_| anyhow::anyhow!("Native agent service lock failed"))?;
    if server.is_none() {
      *server = Some(self.native_machines().serve_agents()?);
    }
    Ok(())
  }

  #[cfg(unix)]
  pub async fn control_native_machine(
    &self,
    id: &str,
  ) -> anyhow::Result<::engine::machines::native::sessions::remote::control::Control> {
    ::engine::machines::native::sessions::remote::control::connect(&self.machines(), id).await
  }

  #[cfg(unix)]
  pub async fn capture_native_machine(&self, id: &str) -> anyhow::Result<crate::MachineFrame> {
    ::engine::machines::native::sessions::remote::capture(&self.machines(), id).await
  }

  pub async fn list_machines(&self, actor: MachineActor) -> anyhow::Result<Vec<MachineStatus>> {
    let mut rows = self.native_machines().list_windows(actor).await?;
    rows.extend(self.machines().list_non_windows(actor).await?);
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
      let service = self
        .virtual_machines
        .lock()
        .map_err(|_| anyhow::anyhow!("VZ service lock failed"))?
        .clone();
      if let Some(service) = service {
        for row in &mut rows {
          if row.machine.guest != model::GuestOs::Windows
            && (row.state == "Unavailable"
              || row.machine.runtime == Some(model::MachineRuntime::Virtualization))
          {
            if let Some(status) = service.status(&row.machine.id, actor).await? {
              use ::engine::machines::vz::State;
              let (state, transitional) = match status.state {
                State::Stopped => ("Stopped", false),
                State::Running => ("Running", false),
                State::Paused => ("Paused", false),
                State::Starting => ("Starting", true),
                State::Pausing => ("Pausing", true),
                State::Resuming => ("Resuming", true),
                State::Stopping => ("Stopping", true),
                State::Error => ("Error", false),
                _ => ("Unavailable", true),
              };
              row.state = state.into();
              row.busy = status.busy || transitional;
              row.progress = if row.machine.guest == model::GuestOs::Linux {
                service
                  .installation(&row.machine.id, actor)?
                  .map(|phase| phase.message().into())
              } else {
                None
              };
              if state == "Unavailable" {
                row.progress = Some(
                  "Controls are unavailable for this VM in this build. Its files are preserved."
                    .into(),
                );
              }
            }
          }
        }
      }
    }
    rows.sort_by_key(|row| row.machine.name.to_lowercase());
    Ok(rows)
  }

  pub async fn create_machine(&self, request: CreateMachine) -> anyhow::Result<Machine> {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    if request.profile == "ubuntu" {
      return ::engine::machines::linux::records::create(&self.machines(), request);
    }
    if self
      .machines()
      .profiles()
      .iter()
      .any(|profile| profile.id == request.profile && profile.guest == model::GuestOs::Windows)
    {
      self.native_machines().create_windows(request)
    } else {
      self.machines().create(request).await
    }
  }
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
impl Host {
  pub fn virtual_machine_owner(&self) -> anyhow::Result<::engine::machines::vz::Owner> {
    let mut service = self
      .virtual_machines
      .lock()
      .map_err(|_| anyhow::anyhow!("VZ service lock failed"))?;
    anyhow::ensure!(service.is_none(), "VZ ownership is already connected");
    let (client, owner) = ::engine::machines::vz::channel();
    *service = Some(::engine::machines::vz::Service::new(
      self.machines(),
      client,
    ));
    Ok(owner)
  }

  pub async fn virtual_machine_lifecycle(
    &self,
    id: &str,
    actor: MachineActor,
    action: ::engine::machines::vz::Action,
  ) -> anyhow::Result<()> {
    let service = self
      .virtual_machines
      .lock()
      .map_err(|_| anyhow::anyhow!("VZ service lock failed"))?
      .clone()
      .ok_or_else(|| anyhow::anyhow!("VZ ownership is not connected"))?;
    service.transition(id, actor, action).await
  }
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
impl Host {
  pub fn virtual_linux_installation(
    &self,
    id: &str,
    actor: MachineActor,
  ) -> anyhow::Result<Option<::engine::machines::linux::progress::Phase>> {
    self
      .virtual_machines
      .lock()
      .map_err(|_| anyhow::anyhow!("VZ service lock failed"))?
      .as_ref()
      .ok_or_else(|| anyhow::anyhow!("VZ ownership is not connected"))?
      .installation(id, actor)
  }

  pub async fn prepare_virtual_linux(
    &self,
    id: &str,
    actor: MachineActor,
    stage: ::engine::machines::vz::Stage,
  ) -> anyhow::Result<::engine::machines::vz::Prepared> {
    let service = self
      .virtual_machines
      .lock()
      .map_err(|_| anyhow::anyhow!("VZ service lock failed"))?
      .clone()
      .ok_or_else(|| anyhow::anyhow!("VZ ownership is not connected"))?;
    service.prepare_linux(id, actor, stage).await
  }

  pub async fn prepare_virtual_linux_tracked(
    &self,
    id: &str,
    actor: MachineActor,
    stage: ::engine::machines::vz::Stage,
    progress: tokio::sync::watch::Sender<crate::VirtualLinuxPreparation>,
  ) -> anyhow::Result<::engine::machines::vz::Prepared> {
    let service = self
      .virtual_machines
      .lock()
      .map_err(|_| anyhow::anyhow!("VZ service lock failed"))?
      .clone()
      .ok_or_else(|| anyhow::anyhow!("VZ ownership is not connected"))?;
    service
      .prepare_linux_tracked(id, actor, stage, progress)
      .await
  }
}
