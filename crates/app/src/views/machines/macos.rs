use super::Machines;
use crate::{
  bridge,
  machines::{self, macos},
};
use futures::SinkExt;
use gpui::{App, Context, WeakEntity};
use host::{MachineActor, VirtualMacLaunch, VirtualMacPhase as Phase};

enum Update {
  Phase(Phase),
  Prepared(Box<anyhow::Result<VirtualMacLaunch>>),
}

enum Started {
  Phase(Phase),
  Finished(anyhow::Result<()>),
}

impl Machines {
  pub(super) fn start_macos(
    &mut self,
    id: String,
    name: String,
    running: bool,
    owned: bool,
    cx: &mut Context<Self>,
  ) {
    if owned && (running || macos::ready(&id, cx)) {
      self.start_virtual(id, name, running, owned, cx);
      return;
    }
    let result = (|| {
      if owned {
        macos::retire(&id, cx)?;
      }
      macos::begin(&id, cx)
    })();
    if let Err(error) = result {
      self.error = Some(format!("{error:#}"));
      cx.notify();
      return;
    }
    self.error = None;
    let host = self.state.host.clone();
    let identity = id.clone();
    let weak = cx.entity().downgrade();
    bridge::stream(
      cx,
      move |mut events| async move {
        let (progress, mut updates) = tokio::sync::watch::channel(Phase::Inspecting);
        let operation = host.prepare_virtual_mac(&identity, MachineActor::Person, progress);
        tokio::pin!(operation);
        loop {
          tokio::select! {
            biased;
            result = &mut operation => {
              let _ = events.send(Update::Prepared(Box::new(result))).await;
              break;
            }
            changed = updates.changed() => {
              if changed.is_ok() { let _ = events.try_send(Update::Phase(*updates.borrow_and_update())); }
            }
          }
        }
      },
      move |update, cx| match update {
        Update::Phase(phase) => {
          macos::phase(&id, phase, cx);
          let _ = weak.update(cx, |_, cx| cx.notify());
        }
        Update::Prepared(result) => {
          let result = (*result)
            .and_then(|launch| macos::admit(launch, cx))
            .and_then(|admission| {
              machines::open(&id, &name, cx)?;
              Ok(admission)
            });
          match result {
            Ok(admission) => {
              let id = id.clone();
              let weak = weak.clone();
              bridge::stream(
                cx,
                move |mut events| async move {
                  let (progress, mut updates) = tokio::sync::watch::channel(Phase::Preparing);
                  let operation = admission.start(progress);
                  tokio::pin!(operation);
                  loop {
                    tokio::select! {
                      biased;
                      result = &mut operation => {
                        let _ = events.send(Started::Finished(result)).await;
                        break;
                      }
                      changed = updates.changed() => {
                        if changed.is_ok() { let _ = events.try_send(Started::Phase(*updates.borrow_and_update())); }
                      }
                    }
                  }
                },
                move |update, cx| match update {
                  Started::Phase(phase) => {
                    macos::phase(&id, phase, cx);
                    let _ = weak.update(cx, |_, cx| cx.notify());
                  }
                  Started::Finished(result) => finish(&id, result, &weak, cx),
                },
                |_| {},
              );
            }
            Err(error) => finish(&id, Err(error), &weak, cx),
          }
        }
      },
      |_| {},
    );
    cx.notify();
  }

  pub(super) fn cancel_macos(&mut self, id: String, cx: &mut Context<Self>) {
    macos::cancelling(&id, cx);
    let host = self.state.host.clone();
    let identity = id.clone();
    let weak = cx.entity().downgrade();
    bridge::run(
      cx,
      async move { host.cancel_virtual_mac(&identity, MachineActor::Person) },
      move |result, cx| {
        if result.is_err() {
          macos::failed_cancel(&id, cx);
        }
        let _ = weak.update(cx, |this, cx| {
          if let Err(error) = result {
            this.error = Some(format!("{error:#}"));
          }
          cx.notify();
        });
      },
    );
    cx.notify();
  }
}

fn finish(id: &str, result: anyhow::Result<()>, weak: &WeakEntity<Machines>, cx: &mut App) {
  macos::finish(id, &result, cx);
  let _ = weak.update(cx, |this, cx| {
    this.error = result.err().map(|error| format!("{error:#}"));
    this.reload(cx);
    cx.notify();
  });
}
