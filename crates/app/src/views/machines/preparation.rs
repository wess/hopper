use super::Machines;
use crate::bridge;
use futures::SinkExt;
use gpui::Context;
use host::{MachineActor, VirtualLinuxPreparation as Phase, VirtualLinuxPrepared};

enum Update {
  Phase(Phase),
  Finished(Box<anyhow::Result<VirtualLinuxPrepared>>),
}

impl Machines {
  pub(super) fn prepare_virtual(&mut self, id: String, name: String, cx: &mut Context<Self>) {
    let host = self.state.host.clone();
    let identity = id.clone();
    let weak = cx.entity().downgrade();
    bridge::stream(
      cx,
      move |mut events| async move {
        let (progress, mut status) = tokio::sync::watch::channel(Phase::Inspecting);
        let operation = host.prepare_virtual_linux_tracked(
          &identity,
          MachineActor::Person,
          host::VirtualLinuxStage::Launch,
          progress,
        );
        tokio::pin!(operation);
        loop {
          tokio::select! {
            biased;
            result = &mut operation => {
              let _ = events.send(Update::Finished(Box::new(result))).await;
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
        let Some(view) = weak.upgrade() else {
          return;
        };
        match update {
          Update::Phase(phase) => view.update(cx, |this, cx| {
            this.busy.insert(id.clone(), phase.message());
            cx.notify();
          }),
          Update::Finished(result) => {
            let result = (*result).and_then(|prepared| crate::machines::admit(prepared, cx));
            match result {
              Err(error) => view.update(cx, |this, cx| {
                this.finish_virtual(&id, &name, false, Err(error), cx)
              }),
              Ok(admission) => {
                let installation = admission.installation();
                view.update(cx, |this, cx| {
                  this.busy.insert(id.clone(), "Starting Ubuntu…".into());
                  cx.notify();
                });
                let id = id.clone();
                let name = name.clone();
                let weak = weak.clone();
                bridge::run(cx, admission.start(), move |result, cx| {
                  if result.is_ok() {
                    if let Some(installation) = installation {
                      crate::machines::watch(installation, cx);
                    }
                  }
                  let _ = weak.update(cx, |this, cx| {
                    this.finish_virtual(&id, &name, false, result, cx)
                  });
                });
              }
            }
          }
        }
      },
      |_| {},
    );
  }
}
