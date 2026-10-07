use crate::{Host, MachineActor};
use model::{CreateMachine, Machine, MachineStatus};

impl Host {
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
