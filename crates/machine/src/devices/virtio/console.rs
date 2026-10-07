//! One named Virtio serial port; guest bytes never become host commands.

use super::queue::Chain;
use crate::dma::{self, Memory, Span};
use anyhow::ensure;
use std::collections::VecDeque;

pub const MULTIPORT: u64 = 1 << 1;
pub const LIMIT: usize = 65536;

pub struct Port {
  name: String,
  announced: bool,
  registered: bool,
  opened: bool,
  controls: VecDeque<Vec<u8>>,
  incoming: VecDeque<u8>,
  outgoing: VecDeque<u8>,
}

pub fn create(name: &str) -> anyhow::Result<Port> {
  ensure!(
    !name.is_empty()
      && name.len() <= 64
      && name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-'),
    "Invalid guest serial port name"
  );
  Ok(Port {
    name: name.to_string(),
    announced: false,
    registered: false,
    opened: false,
    controls: VecDeque::new(),
    incoming: VecDeque::new(),
    outgoing: VecDeque::new(),
  })
}

pub fn opened(port: &Port) -> bool {
  port.registered && port.opened
}

pub fn reset(port: &mut Port) {
  port.announced = false;
  port.registered = false;
  port.opened = false;
  port.controls.clear();
  port.incoming.clear();
  port.outgoing.clear();
}

pub fn send(port: &mut Port, bytes: &[u8]) -> anyhow::Result<()> {
  ensure!(opened(port), "Guest serial port is not open");
  ensure!(
    bytes.len() <= LIMIT - port.incoming.len(),
    "Guest serial input is full"
  );
  port.incoming.extend(bytes);
  Ok(())
}

pub fn receive(port: &mut Port) -> Vec<u8> {
  port.outgoing.drain(..).collect()
}

pub fn ready(port: &Port, index: usize) -> bool {
  match index {
    0 => opened(port) && !port.incoming.is_empty(),
    1 => opened(port) && port.outgoing.len() < LIMIT,
    2 => !port.controls.is_empty(),
    3 => true,
    _ => false,
  }
}

fn packet(event: u16, value: u16) -> Vec<u8> {
  let mut bytes = vec![0; 4];
  bytes.extend(event.to_le_bytes());
  bytes.extend(value.to_le_bytes());
  bytes
}

fn control(port: &mut Port, bytes: &[u8]) -> anyhow::Result<()> {
  ensure!(bytes.len() == 8, "Invalid guest serial control length");
  let id = u32::from_le_bytes(bytes[..4].try_into()?);
  let event = u16::from_le_bytes(bytes[4..6].try_into()?);
  let value = u16::from_le_bytes(bytes[6..].try_into()?);
  ensure!(value <= 1, "Invalid guest serial control value");
  match event {
    0 => {
      if value == 0 {
        reset(port);
      } else if !port.announced {
        port.controls.push_back(packet(1, 1));
        port.announced = true;
      }
    }
    3 if id == 0 && port.announced => {
      if value == 0 {
        port.registered = false;
        port.opened = false;
        port.controls.clear();
        port.incoming.clear();
      } else if !port.registered {
        let mut name = packet(7, 1);
        name.extend(port.name.as_bytes());
        name.push(0);
        port.controls.push_back(name);
        port.controls.push_back(packet(6, 1));
        port.registered = true;
      }
    }
    6 if id == 0 && port.registered => {
      port.opened = value == 1;
      if !port.opened {
        port.incoming.clear();
      }
    }
    _ => anyhow::bail!("Unsupported guest serial control message"),
  }
  ensure!(
    port.controls.len() <= 32,
    "Guest serial control queue is full"
  );
  Ok(())
}

pub fn execute(
  port: &mut Port,
  memory: &mut impl Memory,
  chain: &Chain,
  index: usize,
) -> anyhow::Result<u32> {
  ensure!(index < 4, "Invalid guest serial queue");
  let writable = index.is_multiple_of(2);
  ensure!(
    !chain.buffers.is_empty()
      && chain
        .buffers
        .iter()
        .all(|buffer| buffer.writable == writable),
    "Invalid guest serial buffer direction"
  );
  let spans: Vec<Span> = chain.buffers.iter().map(|buffer| buffer.span).collect();
  let length = dma::length(&spans);
  ensure!(length > 0, "Empty guest serial buffer");
  match index {
    0 => {
      ensure!(opened(port), "Guest serial port is not open");
      let count = length.min(port.incoming.len() as u64) as usize;
      let bytes: Vec<_> = port.incoming.iter().take(count).copied().collect();
      dma::write(memory, &spans, 0, &bytes)?;
      port.incoming.drain(..count);
      Ok(count as u32)
    }
    1 => {
      ensure!(opened(port), "Guest serial port is not open");
      ensure!(
        length <= (LIMIT - port.outgoing.len()) as u64,
        "Guest serial output is full"
      );
      let mut bytes = vec![0; length as usize];
      dma::read(memory, &spans, 0, &mut bytes)?;
      port.outgoing.extend(bytes);
      Ok(0)
    }
    2 => {
      let Some(bytes) = port.controls.front() else {
        return Ok(0);
      };
      ensure!(
        length >= bytes.len() as u64,
        "Guest serial control buffer is too small"
      );
      dma::write(memory, &spans, 0, bytes)?;
      let count = bytes.len();
      port.controls.pop_front();
      Ok(count as u32)
    }
    3 => {
      ensure!(length == 8, "Invalid guest serial control length");
      let mut bytes = [0; 8];
      dma::read(memory, &spans, 0, &mut bytes)?;
      control(port, &bytes)?;
      Ok(0)
    }
    _ => unreachable!(),
  }
}
