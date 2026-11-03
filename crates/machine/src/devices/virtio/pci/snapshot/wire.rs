//! HVT1 transport metadata; backend contents and storage still need separate records.

use super::{msix, queue, Device, SavedQueue, State};
use crate::devices::pci;
use anyhow::ensure;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

pub const MAX: usize = 8192;

pub fn encode(saved: &State) -> anyhow::Result<Zeroizing<Vec<u8>>> {
  ensure!(
    (1..=5).contains(&saved.kind)
      && (1..=4).contains(&saved.queues.len())
      && saved.isr <= 3
      && saved.fault.as_ref().is_none_or(|fault| fault.len() <= 512),
    "Transport record dimensions are unsupported"
  );
  let mut bytes = Zeroizing::new(Vec::from(b"HVT1"));
  bytes.push(saved.kind);
  bytes.extend_from_slice(&saved.offered.to_le_bytes());
  bytes.extend_from_slice(&u16::try_from(saved.window_offset)?.to_le_bytes());
  bytes.push(saved.queues.len() as u8);
  let pci = pci::wire::encode(&saved.pci)?;
  bytes.extend_from_slice(&(pci.len() as u16).to_le_bytes());
  bytes.extend_from_slice(&pci);
  bytes.extend_from_slice(&saved.common);
  bytes.extend_from_slice(&saved.driver_features.to_le_bytes());
  bytes.push(saved.unsupported as u8);
  for queue in &saved.queues {
    bytes.extend_from_slice(&queue.registers);
    bytes.push(queue.queue.is_some() as u8);
    if let Some(queue) = &queue.queue {
      bytes.extend_from_slice(&queue::snapshot::wire::encode(queue)?);
    }
  }
  bytes.push(saved.isr);
  bytes.extend_from_slice(&saved.window);
  bytes.extend_from_slice(&saved.completed.to_le_bytes());
  bytes.extend_from_slice(
    &saved
      .fault
      .as_ref()
      .map_or(u16::MAX, |fault| fault.len() as u16)
      .to_le_bytes(),
  );
  if let Some(fault) = &saved.fault {
    bytes.extend_from_slice(fault.as_bytes());
  }
  let msix = msix::wire::encode(&saved.msix)?;
  bytes.extend_from_slice(&(msix.len() as u16).to_le_bytes());
  bytes.extend_from_slice(&msix);
  let checksum = Sha256::digest(&*bytes);
  bytes.extend_from_slice(&checksum);
  ensure!(bytes.len() <= MAX, "Transport record exceeds its bound");
  Ok(bytes)
}

/// Consume old transport metadata, preserving only its separately captured backend.
/// Matching restored RAM is checked later by restore_transport before mutation.
pub fn reopen(bytes: &[u8], saved: State) -> anyhow::Result<State> {
  let layout = Layout {
    kind: saved.kind,
    offered: saved.offered,
    pci: &saved.pci,
    window_offset: saved.window_offset,
    queues: saved.queues.len(),
    vectors: msix::count(&saved.msix),
  };
  let mut reopened = read(bytes, layout)?;
  reopened.backend = saved.backend;
  Ok(reopened)
}

/// Decode transport metadata against fresh owned hardware without capturing its backend.
/// The returned state has no backend until its separate record is decoded.
pub fn decode(bytes: &[u8], expected: &Device) -> anyhow::Result<State> {
  read(
    bytes,
    Layout {
      kind: super::kind(&expected.backend),
      offered: super::super::features(expected),
      pci: &expected.pci,
      window_offset: expected.window_offset,
      queues: expected.queues.len(),
      vectors: msix::count(&expected.msix),
    },
  )
}

struct Layout<'a> {
  kind: u8,
  offered: u64,
  pci: &'a pci::Function,
  window_offset: usize,
  queues: usize,
  vectors: usize,
}

fn read(bytes: &[u8], layout: Layout<'_>) -> anyhow::Result<State> {
  ensure!(
    (32..=MAX).contains(&bytes.len()),
    "Transport record size is invalid"
  );
  let end = bytes.len() - 32;
  ensure!(
    Sha256::digest(&bytes[..end])[..] == bytes[end..],
    "Transport record checksum is invalid"
  );
  let mut input = Reader(&bytes[..end]);
  ensure!(
    input.take(4)? == b"HVT1",
    "Transport record format is unsupported"
  );
  let kind = input.byte()?;
  let offered = input.number()?;
  let window_offset = input.word()? as usize;
  let count = input.byte()? as usize;
  ensure!(
    kind == layout.kind
      && offered == layout.offered
      && window_offset == layout.window_offset
      && (1..=4).contains(&count)
      && count == layout.queues,
    "Transport record layout is incompatible"
  );
  let count = input.word()? as usize;
  ensure!(
    count <= pci::wire::MAX,
    "Transport PCI record exceeds its bound"
  );
  let pci = pci::wire::decode(input.take(count)?, layout.pci)?;
  let common = input.take(56)?.try_into()?;
  let driver_features = input.number()?;
  let unsupported = input.boolean()?;
  let mut queues = Vec::with_capacity(layout.queues);
  for _ in 0..layout.queues {
    let registers = input.take(32)?.try_into()?;
    let queue = if input.boolean()? {
      Some(queue::snapshot::wire::decode(
        input.take(queue::snapshot::wire::SIZE)?,
      )?)
    } else {
      None
    };
    queues.push(SavedQueue { registers, queue });
  }
  let isr = input.byte()?;
  ensure!(isr <= 3, "Transport ISR bits are invalid");
  let window = input.take(20)?.try_into()?;
  let completed = input.number()?;
  let count = input.word()? as usize;
  let fault = if count == u16::MAX as usize {
    None
  } else {
    ensure!(count <= 512, "Transport fault exceeds its bound");
    Some(std::str::from_utf8(input.take(count)?)?.to_owned())
  };
  let count = input.word()? as usize;
  ensure!(
    count <= msix::wire::MAX,
    "Transport MSI-X record exceeds its bound"
  );
  let msix = msix::wire::decode(input.take(count)?)?;
  ensure!(
    msix::count(&msix) == layout.vectors && input.0.is_empty(),
    "Transport MSI-X layout or trailing data is invalid"
  );
  Ok(State {
    backend: None,
    kind,
    offered,
    pci,
    common,
    driver_features,
    unsupported,
    queues,
    isr,
    window,
    window_offset,
    completed,
    fault,
    msix,
  })
}

struct Reader<'a>(&'a [u8]);
impl<'a> Reader<'a> {
  fn take(&mut self, count: usize) -> anyhow::Result<&'a [u8]> {
    ensure!(count <= self.0.len(), "Transport record is truncated");
    let (value, rest) = self.0.split_at(count);
    self.0 = rest;
    Ok(value)
  }
  fn byte(&mut self) -> anyhow::Result<u8> {
    Ok(self.take(1)?[0])
  }
  fn word(&mut self) -> anyhow::Result<u16> {
    Ok(u16::from_le_bytes(self.take(2)?.try_into()?))
  }
  fn number(&mut self) -> anyhow::Result<u64> {
    Ok(u64::from_le_bytes(self.take(8)?.try_into()?))
  }
  fn boolean(&mut self) -> anyhow::Result<bool> {
    let value = self.byte()?;
    ensure!(value <= 1, "Transport boolean is invalid");
    Ok(value == 1)
  }
}
