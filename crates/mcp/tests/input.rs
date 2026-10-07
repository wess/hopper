#[allow(dead_code)]
#[path = "../src/machines/input.rs"]
mod input;

use model::{native::InputDevice, MachineInput};

#[test]
fn invalid_input_is_rejected_before_claiming_a_guest() {
  for keys in ["", "ctrl++a", "a+a", "ctrl+control", "unknown", "+"] {
    assert!(input::packet(MachineInput::Key { keys: keys.into() }).is_err());
  }
  assert!(input::packet(MachineInput::Text {
    text: "hello".into()
  })
  .is_err());
  for (x, y, button) in [
    (32768, 0, None),
    (0, 32768, None),
    (0, 0, Some("other".into())),
  ] {
    assert!(input::packet(MachineInput::Pointer { x, y, button }).is_err());
  }
}

#[test]
fn shifted_symbols_press_and_release_the_modifier_with_the_key() {
  let (device, events) = input::packet(MachineInput::Key { keys: "!".into() }).unwrap();
  assert!(matches!(device, InputDevice::Keyboard));
  assert_eq!(
    events
      .iter()
      .map(|event| (event.kind, event.code, event.value))
      .collect::<Vec<_>>(),
    [
      (1, 42, 1),
      (1, 2, 1),
      (0, 0, 0),
      (1, 2, 0),
      (1, 42, 0),
      (0, 0, 0)
    ]
  );
  let (_, explicit) = input::packet(MachineInput::Key {
    keys: "shift+!".into(),
  })
  .unwrap();
  assert_eq!(
    serde_json::to_value(explicit).unwrap(),
    serde_json::to_value(events).unwrap()
  );
}
