use crate::{Host, MachineActor};
use engine::machines::native::snapshot::{Entry, Policy};

impl Host {
  pub async fn restore_native_machine_snapshot(
    &self,
    id: &str,
    generation: u64,
    snapshot: &str,
  ) -> anyhow::Result<engine::machines::native::sessions::Restored> {
    engine::machines::native::sessions::remote::restoration::restore(
      &self.machines(),
      id,
      generation,
      snapshot,
    )
    .await
  }

  pub async fn capture_native_machine_snapshot(
    &self,
    id: &str,
    generation: u64,
    snapshot: &str,
    name: &str,
  ) -> anyhow::Result<()> {
    engine::machines::native::sessions::remote::snapshots::capture(
      &self.machines(),
      id,
      generation,
      snapshot,
      name,
    )
    .await
  }

  pub fn native_machine_snapshots(
    &self,
    id: &str,
    actor: MachineActor,
  ) -> anyhow::Result<Vec<Entry>> {
    let manager = self.machines();
    let machine = manager.machine(id, actor)?;
    let policy = match actor {
      MachineActor::Agent => Policy::agent(manager, id, machine.agent_generation)?,
      MachineActor::Person => Policy::person(manager, id)?,
    };
    policy.inventory()
  }

  /// Agent connections retain the original generation across upstream waits.
  pub fn native_machine_snapshots_with_generation(
    &self,
    id: &str,
    generation: u64,
  ) -> anyhow::Result<Vec<Entry>> {
    Policy::agent(self.machines(), id, generation)?.inventory()
  }
}
