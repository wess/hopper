use super::{native, Machines};
use crate::bridge;
use futures::SinkExt;
use gpui::Context;
use host::{MachineActor, MachinePhase};

enum Update {
  Phase(MachinePhase),
  Finished(anyhow::Result<()>),
}

impl Machines {
  pub(super) fn start_windows(
    &mut self,
    id: String,
    name: String,
    running: bool,
    cx: &mut Context<Self>,
  ) {
    self.busy.insert(
      id.clone(),
      if running {
        "Stopping…"
      } else {
        "Preparing Windows…"
      }
      .into(),
    );
    self.error = None;
    let host = self.state.host.clone();
    let service = host.clone();
    let identity = id.clone();
    let view = cx.entity().downgrade();
    bridge::stream(
      cx,
      move |mut events| async move {
        if running {
          let result = service
            .native_machines()
            .stop(&identity, MachineActor::Person)
            .await;
          let _ = events.send(Update::Finished(result)).await;
          return;
        }
        let (progress, mut status) = tokio::sync::watch::channel(MachinePhase::Inspecting);
        let sessions = service.native_machines();
        let operation = sessions.start_windows(&identity, progress);
        tokio::pin!(operation);
        loop {
          tokio::select! {
            biased;
            result = &mut operation => {
              let _ = events.send(Update::Finished(result)).await;
              break;
            }
            changed = status.changed() => {
              if changed.is_ok() {
                let phase = *status.borrow_and_update();
                let _ = events.try_send(Update::Phase(phase));
              }
            }
          }
        }
      },
      move |update, cx| {
        let _ = view.update(cx, |this, cx| {
          match update {
            Update::Phase(phase) => {
              let label = match phase {
                MachinePhase::Acquiring(_) => "Downloading Windows…",
                MachinePhase::Inspecting => "Checking Windows installer…",
                MachinePhase::Accounts => "Preparing guest accounts…",
                MachinePhase::Media(_) => "Preparing installation media…",
                MachinePhase::Disk => "Preparing Windows disk…",
                MachinePhase::Starting => "Starting Windows setup…",
                MachinePhase::Installing(_) => "Installing Windows…",
                MachinePhase::SystemBoot => "Starting Windows…",
              };
              this.busy.insert(id.clone(), label.into());
            }
            Update::Finished(result) => {
              this.busy.remove(&id);
              let result = result.and_then(|()| {
                if running {
                  Ok(())
                } else {
                  native::open(host.clone(), id.clone(), name.clone(), cx)
                }
              });
              this.error = result.err().map(|error| format!("{error:#}"));
              this.reload(cx);
            }
          }
          cx.notify();
        });
      },
      |_| {},
    );
    cx.notify();
  }
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
impl Machines {
  pub(super) fn start_virtual(
    &mut self,
    id: String,
    name: String,
    running: bool,
    owned: bool,
    cx: &mut Context<Self>,
  ) {
    self.busy.insert(
      id.clone(),
      if running {
        "Stopping…"
      } else if !owned {
        "Preparing Linux installer…"
      } else {
        "Starting…"
      }
      .into(),
    );
    self.error = None;
    let host = self.state.host.clone();
    let identity = id.clone();
    let weak = cx.entity().downgrade();
    if !running && !owned {
      bridge::run(
        cx,
        async move {
          host
            .prepare_virtual_linux(
              &identity,
              MachineActor::Person,
              host::VirtualLinuxStage::Unattended,
            )
            .await
        },
        move |result, cx| {
          let Some(view) = weak.upgrade() else {
            return;
          };
          let result = result.and_then(|prepared| crate::machines::admit(prepared, cx));
          match result {
            Err(error) => view.update(cx, |this, cx| {
              this.finish_virtual(&id, &name, false, Err(error), cx)
            }),
            Ok(admission) => {
              view.update(cx, |this, cx| {
                this.busy.insert(id.clone(), "Starting Linux…".into());
                cx.notify();
              });
              bridge::run(cx, admission.start(), move |result, cx| {
                let _ = weak.update(cx, |this, cx| {
                  this.finish_virtual(&id, &name, false, result, cx)
                });
              });
            }
          }
        },
      );
      cx.notify();
      return;
    }
    bridge::run(
      cx,
      async move {
        host
          .virtual_machine_lifecycle(
            &identity,
            MachineActor::Person,
            if running {
              host::VirtualMachineAction::Stop
            } else {
              host::VirtualMachineAction::Start
            },
          )
          .await
      },
      move |result, cx| {
        let _ = weak.update(cx, |this, cx| {
          this.finish_virtual(&id, &name, running, result, cx);
        });
      },
    );
    cx.notify();
  }
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
impl Machines {
  fn finish_virtual(
    &mut self,
    id: &str,
    name: &str,
    running: bool,
    result: anyhow::Result<()>,
    cx: &mut Context<Self>,
  ) {
    self.busy.remove(id);
    let result = result.and_then(|()| {
      if running {
        Ok(())
      } else {
        crate::machines::open(id, name, cx)
      }
    });
    self.error = result.err().map(|error| format!("{error:#}"));
    self.reload(cx);
    cx.notify();
  }
}
