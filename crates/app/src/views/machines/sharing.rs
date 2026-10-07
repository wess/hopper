use super::Machines;
use gpui::{Context, IntoElement, ParentElement, PathPromptOptions, SharedString};
use guise::prelude::*;
use model::{MachineFolder, MachineStatus};
use std::future::Future;

impl Machines {
  fn folder_operation(
    &mut self,
    id: String,
    future: impl Future<Output = anyhow::Result<()>> + Send + 'static,
    saving: bool,
    cx: &mut Context<Self>,
  ) {
    if crate::machines::owns(&id, cx) {
      if let Err(error) = crate::machines::retire(&id, cx) {
        self.error = Some(format!("{error:#}"));
        self.reload(cx);
        return;
      }
    }
    self
      .busy
      .insert(id.clone(), "Saving shared folders…".into());
    self.error = None;
    cx.notify();
    let weak = cx.entity().downgrade();
    super::bridge::run(cx, future, move |result, cx| {
      if let Some(view) = weak.upgrade() {
        view.update(cx, |this, cx| {
          this.busy.remove(&id);
          if let Err(error) = result {
            this.error = Some(format!("{error:#}"));
          } else if saving
            && this
              .folder_draft
              .as_ref()
              .is_some_and(|(draft_id, _)| draft_id == &id)
          {
            this.folder_draft = None;
          }
          this.reload(cx);
          cx.notify();
        });
      }
    });
  }

  fn choose_folder(&mut self, id: String, cx: &mut Context<Self>) {
    let paths = cx.prompt_for_paths(PathPromptOptions {
      files: false,
      directories: true,
      multiple: false,
      prompt: Some("Choose shared folder".into()),
    });
    cx.spawn(async move |this, cx| {
      if let Ok(Ok(Some(paths))) = paths.await {
        if let Some(path) = paths.first() {
          let name: String = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .chars()
            .map(|ch| {
              if ch.is_ascii_alphanumeric() || "._-".contains(ch) {
                ch
              } else {
                '_'
              }
            })
            .take(64)
            .collect();
          let path = path.to_string_lossy().into_owned();
          let _ = this.update(cx, |this, cx| {
            this
              .folder_name
              .update(cx, |input, cx| input.set_text(&name, cx));
            this.folder_draft = Some((id, path));
            this.folder_read_only = true;
            cx.notify();
          });
        }
      }
    })
    .detach();
  }

  pub(super) fn sharing(
    &self,
    row: &MachineStatus,
    busy: bool,
    cx: &mut Context<Self>,
  ) -> impl IntoElement {
    let id = row.machine.id.clone();
    let setting = self.folders.get(&id);
    let folders = setting.and_then(|value| value.as_ref().ok());
    let stopped = matches!(
      row.state.as_str(),
      "Stopped" | "Not created" | "Ready to start" | "Setup required" | "Deployment finished"
    );
    let editable = stopped && !busy && folders.is_some();
    let chooser_id = id.clone();
    let mut view = Stack::new().gap(Size::Xs).child(
      Group::new()
        .gap(Size::Sm)
        .child(Text::new("Shared folders").size(Size::Xs))
        .child(
          Button::new(
            SharedString::from(format!("choose-folder-{id}")),
            "Add folder…",
          )
          .size(Size::Sm)
          .variant(Variant::Subtle)
          .disabled(!editable || folders.is_some_and(|folders| folders.len() >= 16))
          .on_click(cx.listener(move |this, _, _, cx| this.choose_folder(chooser_id.clone(), cx))),
        ),
    );
    match setting {
      Some(Err(error)) => {
        view = view.child(Text::new(format!("Shared folders unavailable: {error}")).size(Size::Xs))
      }
      None => view = view.child(Text::new("Loading shared folders…").size(Size::Xs).dimmed()),
      Some(Ok(folders)) => {
        if folders.is_empty() {
          view = view.child(
            Text::new("Choose a folder to make it available in this guest.")
              .size(Size::Xs)
              .dimmed(),
          );
        }
        for folder in folders {
          let toggle_id = id.clone();
          let remove_id = id.clone();
          let toggle_name = folder.name.clone();
          let remove_name = folder.name.clone();
          let read_only = folder.read_only;
          view = view.child(
            Stack::new()
              .gap(Size::Xs)
              .child(
                Text::new(format!(
                  "{} · {}",
                  folder.name,
                  if read_only {
                    "Read only"
                  } else {
                    "Read and write"
                  }
                ))
                .size(Size::Xs),
              )
              .child(Text::new(folder.path.clone()).size(Size::Xs).dimmed())
              .child(
                Group::new()
                  .gap(Size::Sm)
                  .child(Text::new("Allow changes").size(Size::Xs))
                  .child(
                    Switch::new(SharedString::from(format!(
                      "folder-mode-{id}-{}",
                      folder.name
                    )))
                    .size(Size::Sm)
                    .checked(!read_only)
                    .disabled(!editable)
                    .on_change(cx.listener(move |this, _, _, cx| {
                      let host = this.state.host.clone();
                      let identity = toggle_id.clone();
                      let name = toggle_name.clone();
                      this.folder_operation(
                        toggle_id.clone(),
                        async move {
                          host.set_virtual_machine_folder_read_only(&identity, &name, !read_only)
                        },
                        false,
                        cx,
                      );
                    })),
                  )
                  .child(
                    Button::new(
                      SharedString::from(format!("remove-folder-{id}-{}", folder.name)),
                      "Remove",
                    )
                    .size(Size::Sm)
                    .variant(Variant::Subtle)
                    .disabled(!editable)
                    .on_click(cx.listener(move |this, _, _, cx| {
                      let host = this.state.host.clone();
                      let identity = remove_id.clone();
                      let name = remove_name.clone();
                      this.folder_operation(
                        remove_id.clone(),
                        async move { host.remove_virtual_machine_folder(&identity, &name) },
                        false,
                        cx,
                      );
                    })),
                  ),
              ),
          );
        }
      }
    }
    if let Some((draft_id, path)) = self
      .folder_draft
      .as_ref()
      .filter(|(draft_id, _)| draft_id == &id)
    {
      let draft_id = draft_id.clone();
      let path = path.clone();
      view = view.child(
        Stack::new()
          .gap(Size::Sm)
          .child(Text::new(path.clone()).size(Size::Xs).dimmed())
          .child(self.folder_name.clone())
          .child(
            Group::new()
              .gap(Size::Sm)
              .child(Text::new("Allow changes").size(Size::Xs))
              .child(
                Switch::new("new-folder-mode")
                  .size(Size::Sm)
                  .checked(!self.folder_read_only)
                  .disabled(!editable)
                  .on_change(cx.listener(|this, _, _, cx| {
                    this.folder_read_only = !this.folder_read_only;
                    cx.notify();
                  })),
              ),
          )
          .child(
            Group::new()
              .gap(Size::Sm)
              .child(
                Button::new("save-shared-folder", "Share folder")
                  .size(Size::Sm)
                  .disabled(!editable)
                  .on_click(cx.listener(move |this, _, _, cx| {
                    let folder = MachineFolder {
                      name: this.folder_name.read(cx).text().trim().into(),
                      path: path.clone(),
                      read_only: this.folder_read_only,
                    };
                    let host = this.state.host.clone();
                    let identity = draft_id.clone();
                    this.folder_operation(
                      draft_id.clone(),
                      async move { host.add_virtual_machine_folder(&identity, folder) },
                      true,
                      cx,
                    );
                  })),
              )
              .child(
                Button::new("cancel-shared-folder", "Cancel")
                  .size(Size::Sm)
                  .variant(Variant::Subtle)
                  .on_click(cx.listener(|this, _, _, cx| {
                    this.folder_draft = None;
                    cx.notify();
                  })),
              ),
          ),
      );
    }
    view.child(Text::new(if !stopped {
      "Shut down this VM before changing shared folders."
    } else if row.machine.guest == model::GuestOs::Macos {
      "Changes apply on next start. macOS mounts shared folders automatically."
    } else {
      "Changes apply on next start. New Ubuntu guests show these folders in Shared in your home folder."
    }).size(Size::Xs).dimmed())
  }
}
