use super::{input, Viewer};
use gpui::{Context, MouseButton, Pixels, Point, Window};
use model::native::{InputDevice, InputEvent};

impl Viewer {
  pub(super) fn pointer(
    &mut self,
    point: Point<Pixels>,
    button: Option<(MouseButton, bool)>,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) {
    let Some(image) = &self.image else {
      return;
    };
    let size = image.size(0);
    let bounds = self.viewport.get();
    let Some([x, y]) = input::position(
      [
        f32::from(bounds.origin.x),
        f32::from(bounds.origin.y),
        f32::from(bounds.size.width),
        f32::from(bounds.size.height),
      ],
      [size.width.0 as f32, size.height.0 as f32],
      [f32::from(point.x), f32::from(point.y)],
    ) else {
      if button.is_some_and(|(_, down)| !down) {
        self.release();
      }
      return;
    };
    if button.is_some_and(|(_, down)| down) {
      self.enabled = true;
      window.focus(&self.focus);
    }
    let mut events = vec![
      InputEvent {
        kind: 3,
        code: 0,
        value: x,
      },
      InputEvent {
        kind: 3,
        code: 1,
        value: y,
      },
    ];
    if let Some((button, down)) = button {
      let code = match button {
        MouseButton::Left => 272,
        MouseButton::Right => 273,
        MouseButton::Middle => 274,
        _ => return,
      };
      events.push(input::key(code, i32::from(down)));
    }
    self.send_input(InputDevice::Tablet, events);
    cx.notify();
  }
}
