//! Bounded installation events; deployment completion never means desktop readiness.

use anyhow::{ensure, Context};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Phase {
  Files,
  Pe,
  Drivers,
  Media,
  Image,
  Letters,
  Partition,
  Apply,
  OfflineDrivers,
  Provision,
  Recovery,
  Boot,
}

const PHASES: [Phase; 12] = [
  Phase::Files,
  Phase::Pe,
  Phase::Drivers,
  Phase::Media,
  Phase::Image,
  Phase::Letters,
  Phase::Partition,
  Phase::Apply,
  Phase::OfflineDrivers,
  Phase::Provision,
  Phase::Recovery,
  Phase::Boot,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
  Phase(Phase),
  Failed(Phase),
  Deployed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Status {
  #[default]
  Waiting,
  Active(Phase),
  Failed(Phase),
  Deployed,
  Invalid,
}

#[derive(Default)]
pub struct Decoder {
  line: Vec<u8>,
  status: Status,
  poisoned: bool,
}

pub fn status(decoder: &Decoder) -> Status {
  decoder.status
}

fn event(bytes: &[u8]) -> anyhow::Result<Event> {
  ensure!(bytes.ends_with(b"\r"), "Invalid setup event framing");
  let line =
    std::str::from_utf8(&bytes[..bytes.len() - 1]).context("Invalid setup event encoding")?;
  let fields: Vec<_> = line.split(' ').collect();
  ensure!(
    fields.len() == 3 && fields[0] == "HOPPERSETUP/1",
    "Invalid setup event header"
  );
  let index: usize = fields[2].parse().context("Invalid setup event phase")?;
  ensure!(fields[2] == index.to_string(), "Invalid setup event phase");
  let phase = *PHASES.get(index).context("Invalid setup event phase")?;
  match fields[1] {
    "phase" => Ok(Event::Phase(phase)),
    "failed" => Ok(Event::Failed(phase)),
    "deployed" if phase == Phase::Boot => Ok(Event::Deployed),
    _ => anyhow::bail!("Invalid setup event kind"),
  }
}

fn apply(decoder: &mut Decoder, event: Event) -> anyhow::Result<()> {
  decoder.status = match (decoder.status, event) {
    (Status::Waiting, Event::Phase(Phase::Files)) => Status::Active(Phase::Files),
    (Status::Active(current), Event::Phase(next))
      if next == current || next as u8 == current as u8 + 1 =>
    {
      Status::Active(next)
    }
    (Status::Active(current), Event::Failed(phase)) if current == phase => Status::Failed(phase),
    (Status::Active(Phase::Boot), Event::Deployed) => Status::Deployed,
    _ => anyhow::bail!("Out-of-order setup event"),
  };
  Ok(())
}

pub fn feed(decoder: &mut Decoder, bytes: &[u8]) -> anyhow::Result<Vec<Event>> {
  ensure!(!decoder.poisoned, "Setup event stream is invalid");
  let result = (|| {
    ensure!(bytes.len() <= 4096, "Setup event batch is too large");
    let mut events = Vec::new();
    for byte in bytes {
      ensure!(
        !matches!(decoder.status, Status::Failed(_) | Status::Deployed),
        "Setup event received after completion"
      );
      if *byte == b'\n' {
        let event = event(&decoder.line)?;
        apply(decoder, event)?;
        events.push(event);
        decoder.line.clear();
      } else {
        ensure!(decoder.line.len() < 64, "Setup event line is too long");
        decoder.line.push(*byte);
      }
    }
    Ok(events)
  })();
  if result.is_err() {
    decoder.poisoned = true;
    decoder.status = Status::Invalid;
    decoder.line.clear();
  }
  result
}

pub fn finish(decoder: &mut Decoder) -> anyhow::Result<Status> {
  if decoder.poisoned
    || !decoder.line.is_empty()
    || !matches!(decoder.status, Status::Failed(_) | Status::Deployed)
  {
    decoder.poisoned = true;
    decoder.status = Status::Invalid;
    decoder.line.clear();
    anyhow::bail!("Setup event stream ended without a complete result");
  }
  Ok(decoder.status)
}
