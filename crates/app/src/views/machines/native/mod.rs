mod frame;
mod input;
mod keyboard;
mod pointer;
mod render;

use crate::{bridge, theme};
use gpui::prelude::*;
use gpui::{
  div, img, px, size, App, Bounds, Context, FocusHandle, Global, RenderImage, Task,
  TitlebarOptions, Window, WindowBounds, WindowHandle, WindowOptions,
};
use guise::prelude::*;
use host::{Host, MachineActor, MachineState};
use model::native::Command;
use std::{cell::Cell, collections::BTreeMap, rc::Rc, sync::Arc, time::Duration};

#[derive(Default)]
struct Viewers(BTreeMap<String, WindowHandle<Viewer>>);
impl Global for Viewers {}

pub(super) fn open(host: Arc<Host>, id: String, name: String, cx: &mut App) -> anyhow::Result<()> {
  if let Some(window) = cx.default_global::<Viewers>().0.get(&id).copied() {
    if window
      .update(cx, |_, window, _| window.activate_window())
      .is_ok()
    {
      return Ok(());
    }
  }
  let bounds = Bounds::centered(None, size(px(1100.0), px(760.0)), cx);
  let identity = id.clone();
  let window = cx.open_window(
    WindowOptions {
      window_bounds: Some(WindowBounds::Windowed(bounds)),
      window_min_size: Some(size(px(640.0), px(480.0))),
      titlebar: Some(TitlebarOptions {
        title: Some(name.clone().into()),
        ..Default::default()
      }),
      ..Default::default()
    },
    move |window, cx| cx.new(|cx| Viewer::new(host, identity, name, window, cx)),
  )?;
  cx.default_global::<Viewers>().0.insert(id, window);
  cx.activate(true);
  Ok(())
}

struct Viewer {
  host: Arc<Host>,
  id: String,
  name: String,
  state: Option<MachineState>,
  image: Option<Arc<RenderImage>>,
  previous: Option<Arc<RenderImage>>,
  command_error: Option<String>,
  error: Option<String>,
  busy: bool,
  focus: FocusHandle,
  input: input::Input,
  enabled: bool,
  active: bool,
  modifiers: gpui::Modifiers,
  viewport: Rc<Cell<Bounds<gpui::Pixels>>>,
  _poll: Task<()>,
}

impl Viewer {
  fn new(
    host: Arc<Host>,
    id: String,
    name: String,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> Self {
    let focus = cx.focus_handle();
    cx.on_focus_out(&focus, window, |this, _, _, _| this.release())
      .detach();
    cx.observe_window_activation(window, |this, window, cx| {
      this.active = window.is_window_active();
      if !this.active {
        this.release();
      }
      cx.notify();
    })
    .detach();
    let input = input::create(host.clone(), id.clone());
    cx.on_release(|this, cx| {
      this.release();
      cx.default_global::<Viewers>().0.remove(&this.id);
      for image in [this.image.take(), this.previous.take()]
        .into_iter()
        .flatten()
      {
        cx.drop_image(image, None);
      }
    })
    .detach();
    let service = host.clone();
    let identity = id.clone();
    let poll = cx.spawn(async move |view, cx| loop {
      let host = service.clone();
      let id = identity.clone();
      let (send, receive) = futures::channel::oneshot::channel();
      let task = bridge::runtime().spawn(async move {
        let result = async {
          let state = host
            .native_machines()
            .state(&id, MachineActor::Person)
            .await?;
          let image = if matches!(state, Some(MachineState::Running | MachineState::Paused)) {
            Some(frame::render(
              host
                .native_machines()
                .capture(&id, MachineActor::Person)
                .await?,
            )?)
          } else {
            None
          };
          Ok::<_, anyhow::Error>((state, image))
        }
        .await;
        let _ = send.send(result);
      });
      let task = bridge::abort_on_drop(task);
      let Ok(result) = receive.await else {
        break;
      };
      drop(task);
      let Ok(delay) = view.update(cx, |this, cx| {
        match result {
          Ok((state, image)) => {
            this.error = match &state {
              Some(MachineState::Failed(error)) => Some(error.clone()),
              _ => None,
            };
            if this.enabled && !matches!(state, Some(MachineState::Running)) {
              this.release();
            }
            this.state = state;
            if let Some(previous) = this.previous.take() {
              cx.drop_image(previous, None);
            }
            this.previous = this.image.take();
            this.image = image;
          }
          Err(error) => {
            if this.enabled {
              this.release();
            }
            this.error = Some(format!("{error:#}"));
          }
        }
        if this.enabled && this.input.error.borrow().is_some() {
          this.release();
        }
        cx.notify();
        if this.active && matches!(this.state, Some(MachineState::Running)) {
          33
        } else {
          500
        }
      }) else {
        break;
      };
      cx.background_executor()
        .timer(Duration::from_millis(delay))
        .await;
    });
    Self {
      host,
      id,
      name,
      state: None,
      image: None,
      previous: None,
      command_error: None,
      error: None,
      busy: false,
      focus,
      input,
      enabled: false,
      active: window.is_window_active(),
      modifiers: gpui::Modifiers::default(),
      viewport: Rc::new(Cell::new(Bounds::default())),
      _poll: poll,
    }
  }

  fn command(&mut self, command: Command, cx: &mut Context<Self>) {
    if self.busy {
      return;
    }
    self.release();
    self.busy = true;
    self.command_error = None;
    let host = self.host.clone();
    let id = self.id.clone();
    let view = cx.entity().downgrade();
    bridge::run(
      cx,
      async move {
        if matches!(command, Command::Stop {}) {
          host.native_machines().stop(&id, MachineActor::Person).await
        } else {
          host
            .native_machines()
            .request(&id, MachineActor::Person, command)
            .await
            .map(|_| ())
        }
      },
      move |result, cx| {
        let _ = view.update(cx, |this, cx| {
          this.busy = false;
          this.command_error = result.err().map(|error| format!("{error:#}"));
          cx.notify();
        });
      },
    );
    cx.notify();
  }

  fn label(&self) -> &str {
    match &self.state {
      Some(MachineState::Running) => "Running",
      Some(MachineState::Paused) => "Paused",
      Some(MachineState::Stopped(model::native::StopReason::Reset)) => "Restarting Windows…",
      Some(MachineState::Stopped(_)) => "Stopped",
      Some(MachineState::Failed(_)) => "VM stopped with an error",
      None => "Waiting for the guest display…",
    }
  }
}
