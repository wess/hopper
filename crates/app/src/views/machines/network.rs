use super::Machines;
use gpui::{Context, IntoElement, ParentElement, SharedString};
use guise::prelude::*;
use model::MachineStatus;

impl Machines {
  pub(super) fn network(
    &self,
    row: &MachineStatus,
    busy: bool,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let id = row.machine.id.clone();
    let setting = self.networks.get(&id);
    let connected = setting.and_then(|value| value.as_ref().ok()).copied();
    let stopped = matches!(
      row.state.as_str(),
      "Stopped" | "Not created" | "Ready to start" | "Setup required" | "Deployment finished"
    );
    let editable = stopped && !busy && connected.is_some();
    let message = match setting {
      Some(Err(error)) => format!("Network settings unavailable: {error}"),
      None => "Loading network settings…".into(),
      Some(Ok(_)) if !editable => "Shut down this VM before changing its network.".into(),
      Some(Ok(true)) => {
        "NAT connects this guest through your Mac. Changes apply on next start.".into()
      }
      Some(Ok(false)) => {
        "Disconnected: this guest has no network attachment. Changes apply on next start.".into()
      }
    };
    Stack::new()
      .gap(Size::Xs)
      .child(
        Group::new()
          .gap(Size::Sm)
          .child(Text::new("Network connection").size(Size::Xs))
          .child(
            Switch::new(SharedString::from(format!("network-{id}")))
              .size(Size::Sm)
              .checked(connected.unwrap_or(false))
              .disabled(!editable)
              .on_change(cx.listener(move |this, _, _, cx| {
                let Some(connected) = connected else {
                  return;
                };
                if crate::machines::owns(&id, cx) {
                  if let Err(error) = crate::machines::retire(&id, cx) {
                    this.error = Some(format!("{error:#}"));
                    this.reload(cx);
                    return;
                  }
                }
                let host = this.state.host.clone();
                let identity = id.clone();
                this.operate(
                  id.clone(),
                  "Saving network…",
                  async move { host.set_virtual_machine_network(&identity, !connected) },
                  cx,
                );
              })),
          ),
      )
      .child(Text::new(message).size(Size::Xs).dimmed())
  }
}
