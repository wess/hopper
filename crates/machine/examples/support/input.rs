use anyhow::Context;
use machine::devices::virtio::input::{Event, SYN};

pub fn text(text: &[u8]) -> anyhow::Result<Vec<Event>> {
  let mut events = Vec::new();
  let keys = [
    30, 48, 46, 32, 18, 33, 34, 35, 23, 36, 37, 38, 50, 49, 24, 25, 16, 19, 31, 20, 22, 47, 17, 45,
    21, 44,
  ];
  for byte in text {
    let code = match byte {
      b'a'..=b'z' => Some(keys[(byte - b'a') as usize]),
      b'0' => Some(11),
      b'1'..=b'9' => Some((byte - b'1') as u16 + 2),
      b' ' => Some(57),
      b'/' => Some(53),
      b'-' => Some(12),
      b'\r' => Some(28),
      _ => None,
    }
    .context("Unsupported diagnostic key")?;
    events.extend([
      Event {
        kind: 1,
        code,
        value: 1,
      },
      SYN,
      Event {
        kind: 1,
        code,
        value: 0,
      },
      SYN,
    ]);
  }
  Ok(events)
}
