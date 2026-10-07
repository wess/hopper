use super::Machines;
use gpui::{Context, IntoElement, ParentElement, SharedString};
use guise::prelude::*;
use model::MachineStatus;

impl Machines {
  pub(super) fn audio(
    &self,
    row: &MachineStatus,
    busy: bool,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let id = row.machine.id.clone();
    let setting = self.speakers.get(&id);
    let enabled = setting.and_then(|value| value.as_ref().ok()).copied();
    let stopped = matches!(
      row.state.as_str(),
      "Stopped" | "Not created" | "Ready to start" | "Setup required" | "Deployment finished"
    );
    let editable = stopped && !busy && enabled.is_some();
    let message = match setting {
      Some(Err(error)) => format!("Speaker settings unavailable: {error}"),
      None => "Loading speaker settings…".into(),
      Some(Ok(_)) if !editable => "Shut down this VM before changing its speakers.".into(),
      Some(Ok(true)) => "Play guest sound through your Mac. Changes apply on next start.".into(),
      Some(Ok(false)) => {
        "Guest sound is off. Changes apply on next start.".into()
      }
    };
    Stack::new()
      .gap(Size::Xs)
      .child(
        Group::new()
          .gap(Size::Sm)
          .child(Text::new("Speakers").size(Size::Xs))
          .child(
            Switch::new(SharedString::from(format!("speakers-{id}")))
              .size(Size::Sm)
              .checked(enabled.unwrap_or(false))
              .disabled(!editable)
              .on_change(cx.listener(move |this, _, _, cx| {
                let Some(enabled) = enabled else {
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
                  "Saving speakers…",
                  async move { host.set_virtual_machine_speakers(&identity, !enabled) },
                  cx,
                );
              })),
          ),
      )
      .child(Text::new(message).size(Size::Xs).dimmed())
      .child(
        Text::new("Microphone input is not available yet.")
          .size(Size::Xs)
          .dimmed(),
      )
  }
}
