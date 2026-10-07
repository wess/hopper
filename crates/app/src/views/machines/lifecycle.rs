use super::Machines;
use gpui::Context;
use host::{MachineActor, VirtualMachineAction};
use model::native::{Command, Result as Reply};

impl Machines {
  pub(super) fn pause_virtual(
    &mut self,
    id: String,
    windows: bool,
    resume: bool,
    cx: &mut Context<Self>,
  ) {
    let host = self.state.host.clone();
    let identity = id.clone();
    self.operate(
      id,
      if resume { "Resuming…" } else { "Pausing…" },
      async move {
        if windows {
          let command = if resume {
            Command::Resume {}
          } else {
            Command::Pause {}
          };
          let reply = host
            .native_machines()
            .request(&identity, MachineActor::Person, command)
            .await?;
          anyhow::ensure!(
            matches!(
              (resume, reply),
              (true, Reply::Running {}) | (false, Reply::Paused {})
            ),
            "Native VM did not acknowledge its lifecycle transition"
          );
          Ok(())
        } else {
          host
            .virtual_machine_lifecycle(
              &identity,
              MachineActor::Person,
              if resume {
                VirtualMachineAction::Resume
              } else {
                VirtualMachineAction::Pause
              },
            )
            .await
        }
      },
      cx,
    );
  }
}
