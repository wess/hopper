//! HVC1 stores guest serial device buffers; authenticated job sessions are not restored.

use super::{create, packet, Port, LIMIT};
use anyhow::ensure;
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use zeroize::Zeroizing;

pub const MAX: usize = 134000;

fn validate(port: &Port) -> anyhow::Result<()> {
  create(&port.name)?;
  ensure!(
    port.controls.len() <= 32
      && port.incoming.len() <= LIMIT
      && port.outgoing.len() <= LIMIT
      && (!port.registered || port.announced)
      && (!port.opened || port.registered)
      && (port.opened || port.incoming.is_empty()),
    "Console record state is invalid"
  );
  let mut name = packet(7, 1);
  name.extend(port.name.as_bytes());
  name.push(0);
  for control in &port.controls {
    ensure!(
      (control == &packet(1, 1) && port.announced)
        || ((control == &packet(6, 1) || control == &name) && port.registered),
      "Console record control message is invalid"
    );
  }
  Ok(())
}

pub fn encode(port: &Port) -> anyhow::Result<Zeroizing<Vec<u8>>> {
  validate(port)?;
  let mut bytes = Zeroizing::new(Vec::from(b"HVC1"));
  bytes.push(port.name.len() as u8);
  bytes.extend_from_slice(port.name.as_bytes());
  bytes.extend_from_slice(&[
    port.announced as u8,
    port.registered as u8,
    port.opened as u8,
    port.controls.len() as u8,
  ]);
  for control in &port.controls {
    bytes.push(control.len() as u8);
    bytes.extend_from_slice(control);
  }
  for buffer in [&port.incoming, &port.outgoing] {
    bytes.extend_from_slice(&(buffer.len() as u32).to_le_bytes());
    bytes.extend(buffer.iter().copied());
  }
  let checksum = Sha256::digest(&*bytes);
  bytes.extend_from_slice(&checksum);
  ensure!(bytes.len() <= MAX, "Console record exceeds its bound");
  Ok(bytes)
}

pub fn decode(bytes: &[u8]) -> anyhow::Result<Port> {
  ensure!(
    (50..=MAX).contains(&bytes.len()),
    "Console record size is invalid"
  );
  let end = bytes.len() - 32;
  ensure!(
    Sha256::digest(&bytes[..end])[..] == bytes[end..],
    "Console record checksum is invalid"
  );
  let mut input = Reader(&bytes[..end]);
  ensure!(
    input.take(4)? == b"HVC1",
    "Console record format is unsupported"
  );
  let count = input.byte()? as usize;
  ensure!(
    (1..=64).contains(&count),
    "Console record name size is invalid"
  );
  let name = std::str::from_utf8(input.take(count)?)?.to_owned();
  let announced = input.boolean()?;
  let registered = input.boolean()?;
  let opened = input.boolean()?;
  let count = input.byte()? as usize;
  ensure!(count <= 32, "Console record control count is invalid");
  let mut controls = VecDeque::with_capacity(count);
  for _ in 0..count {
    let count = input.byte()? as usize;
    ensure!(
      (8..=73).contains(&count),
      "Console record control size is invalid"
    );
    controls.push_back(input.take(count)?.to_vec());
  }
  let mut buffer = || -> anyhow::Result<VecDeque<u8>> {
    let count = u32::from_le_bytes(input.take(4)?.try_into()?) as usize;
    ensure!(count <= LIMIT, "Console record buffer exceeds its bound");
    Ok(input.take(count)?.iter().copied().collect())
  };
  let incoming = buffer()?;
  let outgoing = buffer()?;
  ensure!(input.0.is_empty(), "Console record has trailing bytes");
  let port = Port {
    name,
    announced,
    registered,
    opened,
    controls,
    incoming,
    outgoing,
  };
  validate(&port)?;
  Ok(port)
}

struct Reader<'a>(&'a [u8]);
impl<'a> Reader<'a> {
  fn take(&mut self, count: usize) -> anyhow::Result<&'a [u8]> {
    ensure!(count <= self.0.len(), "Console record is truncated");
    let (value, rest) = self.0.split_at(count);
    self.0 = rest;
    Ok(value)
  }
  fn byte(&mut self) -> anyhow::Result<u8> {
    Ok(self.take(1)?[0])
  }
  fn boolean(&mut self) -> anyhow::Result<bool> {
    let value = self.byte()?;
    ensure!(value <= 1, "Console record boolean is invalid");
    Ok(value == 1)
  }
}
