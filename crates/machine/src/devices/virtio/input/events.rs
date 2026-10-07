use super::{Event, Input, Kind, SYN};
use crate::{
  devices::virtio::queue::Chain,
  dma::{self, Memory},
};
use anyhow::{ensure, Context};

fn valid(input: &Input, event: Event) -> bool {
  match (event.kind, input.kind) {
    (0, _) => event == SYN,
    (1, Kind::Keyboard) => (1..=255).contains(&event.code) && (0..=2).contains(&event.value),
    (1, Kind::Tablet) => (272..=274).contains(&event.code) && (0..=1).contains(&event.value),
    (3, Kind::Tablet) => event.code <= 1 && (0..=65535).contains(&event.value),
    (2, Kind::Tablet) => event.code == 8,
    _ => false,
  }
}

pub fn enqueue(input: &mut Input, events: &[Event]) -> anyhow::Result<()> {
  ensure!(
    !events.is_empty() && events.last() == Some(&SYN),
    "Input update requires SYN_REPORT"
  );
  ensure!(
    events.len() + input.pending.len() <= 1024,
    "Guest input queue is full"
  );
  ensure!(
    events.iter().all(|event| valid(input, *event)),
    "Unsupported guest input event"
  );
  input.pending.extend(events);
  Ok(())
}

pub fn release(input: &mut Input) {
  // discard unsent transitions; release only keys the guest has actually received.
  let sync = !input.pending.is_empty() || !input.held.is_empty();
  input.pending.clear();
  input.pending.extend(input.held.iter().map(|code| Event {
    kind: 1,
    code: *code,
    value: 0,
  }));
  if sync {
    input.pending.push_back(SYN);
  }
}

pub fn execute(
  input: &mut Input,
  memory: &mut impl Memory,
  chain: &Chain,
  index: usize,
) -> anyhow::Result<u32> {
  if index == 0 {
    ensure!(
      chain.buffers.iter().all(|buffer| buffer.writable),
      "Input event buffer is not writable"
    );
    let spans: Vec<_> = chain.buffers.iter().map(|buffer| buffer.span).collect();
    let event = input.pending.front().context("No pending guest input")?;
    let mut bytes = [0; 8];
    bytes[..2].copy_from_slice(&event.kind.to_le_bytes());
    bytes[2..4].copy_from_slice(&event.code.to_le_bytes());
    bytes[4..].copy_from_slice(&event.value.to_le_bytes());
    dma::write(memory, &spans, 0, &bytes)?;
    if event.kind == 1 {
      if event.value == 0 {
        input.held.remove(&event.code);
      } else {
        input.held.insert(event.code);
      }
    }
    input.pending.pop_front();
    return Ok(8);
  }
  ensure!(
    index == 1 && chain.buffers.iter().all(|buffer| !buffer.writable),
    "Invalid input status buffer"
  );
  let spans: Vec<_> = chain.buffers.iter().map(|buffer| buffer.span).collect();
  let length = dma::length(&spans);
  ensure!(
    length > 0 && length <= 1024 && length.is_multiple_of(8),
    "Invalid input status length"
  );
  for offset in (0..length).step_by(8) {
    let mut bytes = [0; 8];
    dma::read(memory, &spans, offset, &mut bytes)?;
    let kind = u16::from_le_bytes(bytes[..2].try_into()?);
    let code = u16::from_le_bytes(bytes[2..4].try_into()?);
    let value = u32::from_le_bytes(bytes[4..].try_into()?);
    if input.kind == Kind::Keyboard && kind == 17 && code < 3 && value <= 1 {
      input.leds = (input.leds & !(1 << code)) | (value as u8) << code;
    }
  }
  Ok(0)
}
