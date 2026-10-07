use super::{input, Viewer};
use gpui::{Keystroke, Modifiers};
use model::native::{InputDevice, InputEvent};

impl Viewer {
  pub(super) fn release(&mut self) {
    self.enabled = false;
    self.modifiers = Modifiers::default();
    self.input.release();
  }

  pub(super) fn send_input(&mut self, device: InputDevice, events: Vec<InputEvent>) {
    if !self.enabled || !self.active || !matches!(self.state, Some(host::MachineState::Running)) {
      return;
    }
    if !self.input.send(input::events(device, events)) {
      self.release();
      self.command_error = Some("Guest input is busy. Click the display to reconnect".into());
    }
  }

  fn modifier_events(&mut self, next: Modifiers) -> Vec<InputEvent> {
    let previous = self.modifiers;
    self.modifiers = next;
    [
      (29, previous.control, next.control),
      (56, previous.alt, next.alt),
      (42, previous.shift, next.shift),
      (125, previous.platform, next.platform),
    ]
    .into_iter()
    .filter(|(_, old, new)| old != new)
    .map(|(code, _, new)| input::key(code, i32::from(new)))
    .collect()
  }

  pub(super) fn modifiers(&mut self, next: Modifiers) {
    if !self.enabled {
      return;
    }
    let events = self.modifier_events(next);
    if !events.is_empty() {
      self.send_input(InputDevice::Keyboard, events);
    }
  }

  pub(super) fn key(&mut self, key: &Keystroke, value: i32) {
    if !self.enabled {
      return;
    }
    let Some(code) = input::code(&key.key.to_ascii_lowercase()) else {
      return;
    };
    let mut events = self.modifier_events(key.modifiers);
    events.push(input::key(code, value));
    self.send_input(InputDevice::Keyboard, events);
  }
}
