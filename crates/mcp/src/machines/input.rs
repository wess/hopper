use anyhow::{ensure, Context};
use model::{
  native::{keyboard, InputDevice, InputEvent},
  MachineInput,
};

pub(super) async fn native(host: &host::Host, id: &str, input: MachineInput) -> anyhow::Result<()> {
  let (device, events) = packet(input)?;
  let mut control = host.control_native_machine(id).await?;
  let result = control.send(device, events).await;
  let released = control.close().await;
  result?;
  released
}

pub fn packet(input: MachineInput) -> anyhow::Result<(InputDevice, Vec<InputEvent>)> {
  match input {
    MachineInput::Key { keys } => {
      ensure!(
        !keys.is_empty() && keys.len() <= 256,
        "Native key chord exceeds bounds"
      );
      let names: Vec<_> = keys.split('+').map(str::trim).collect();
      ensure!(
        (1..=16).contains(&names.len()) && names.iter().all(|name| !name.is_empty()),
        "Provide 1–16 native key names joined by +"
      );
      let mut codes = Vec::new();
      for name in &names {
        let code = keyboard::code(&name.to_ascii_lowercase())
          .context("Unknown native key name; use US key names joined by +")?;
        ensure!(!codes.contains(&code), "A key chord cannot repeat a key");
        codes.push(code);
      }
      if names
        .iter()
        .any(|name| name.len() == 1 && "!@#$%^&*()_+{}:\"~|<>?".contains(*name))
        && !codes.contains(&42)
      {
        codes.insert(0, 42);
      }
      let mut events: Vec<_> = codes.iter().map(|code| key(*code, 1)).collect();
      events.push(sync());
      events.extend(codes.iter().rev().map(|code| key(*code, 0)));
      events.push(sync());
      Ok((InputDevice::Keyboard, events))
    }
    MachineInput::Pointer { x, y, button } => {
      ensure!(
        x <= 32767 && y <= 32767,
        "Pointer coordinates must be within 0–32767"
      );
      let mut events = vec![
        InputEvent {
          kind: 3,
          code: 0,
          value: (u32::from(x) * 65535 / 32767) as i32,
        },
        InputEvent {
          kind: 3,
          code: 1,
          value: (u32::from(y) * 65535 / 32767) as i32,
        },
      ];
      if let Some(button) = button {
        let code = match button.as_str() {
          "left" => 272,
          "right" => 273,
          "middle" => 274,
          _ => anyhow::bail!("Choose a left, right or middle pointer button"),
        };
        events.extend([key(code, 1), sync(), key(code, 0)]);
      }
      events.push(sync());
      Ok((InputDevice::Tablet, events))
    }
    MachineInput::Text { .. } => anyhow::bail!(
      "Native Windows text input requires guest tools; use key input for US key chords"
    ),
  }
}

fn key(code: u16, value: i32) -> InputEvent {
  InputEvent {
    kind: 1,
    code,
    value,
  }
}
fn sync() -> InputEvent {
  InputEvent {
    kind: 0,
    code: 0,
    value: 0,
  }
}
