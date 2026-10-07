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
    rows.sort_by_key(|row| row.machine.name.to_lowercase());
    Ok(rows)
  }

  pub async fn create_machine(&self, request: CreateMachine) -> anyhow::Result<Machine> {
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
