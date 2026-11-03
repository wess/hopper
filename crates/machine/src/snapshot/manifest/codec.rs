use super::{Binding, Entry, Layout, Manifest, Record, MAX};
use anyhow::ensure;
use sha2::{Digest, Sha256};

pub(super) fn encode(saved: &Manifest) -> Vec<u8> {
  let mut bytes = Vec::from(b"HSM2");
  bytes.extend_from_slice(&saved.binding.machine);
  bytes.extend_from_slice(&saved.binding.snapshot);
  bytes.extend_from_slice(&saved.binding.configuration);
  bytes.push(saved.layout.cpus);
  for (address, size) in saved.layout.banks {
    bytes.extend_from_slice(&address.to_le_bytes());
    bytes.extend_from_slice(&size.to_le_bytes());
  }
  bytes.extend_from_slice(&saved.layout.devices);
  bytes.push(saved.entries.len() as u8);
  for entry in &saved.entries {
    let (kind, slot) = match entry.record {
      Record::Cpu(slot) => (1, slot),
      Record::Software => (2, 0),
      Record::Uart => (3, 0),
      Record::Rtc => (9, 0),
      Record::Flash => (4, 0),
      Record::Interrupts => (5, 0),
      Record::Memory(slot) => (6, slot),
      Record::Transport(slot) => (7, slot),
      Record::Backend(slot) => (8, slot),
    };
    bytes.extend_from_slice(&[kind, slot]);
    bytes.extend_from_slice(&entry.length.to_le_bytes());
    bytes.extend_from_slice(&entry.checksum);
  }
  let checksum = Sha256::digest(&bytes);
  bytes.extend_from_slice(&checksum);
  bytes
}

pub(super) fn inspect(bytes: &[u8]) -> anyhow::Result<Manifest> {
  ensure!(
    !bytes.starts_with(b"HSM1"),
    "Snapshot predates native clock state; preserve it for explicit migration"
  );
  ensure!(
    (157..=MAX).contains(&bytes.len()) && &bytes[..4] == b"HSM2",
    "Unknown or malformed snapshot manifest"
  );
  let end = bytes.len() - 32;
  ensure!(
    Sha256::digest(&bytes[..end])[..] == bytes[end..],
    "Snapshot manifest checksum mismatch"
  );
  let binding = Binding {
    machine: bytes[4..20].try_into()?,
    snapshot: bytes[20..36].try_into()?,
    configuration: bytes[36..68].try_into()?,
  };
  let mut banks = [(0, 0); 3];
  for (bank, bytes) in banks.iter_mut().zip(bytes[69..117].as_chunks::<16>().0) {
    *bank = (
      u64::from_le_bytes(bytes[..8].try_into()?),
      u64::from_le_bytes(bytes[8..].try_into()?),
    );
  }
  let found = Layout {
    cpus: bytes[68],
    banks,
    devices: bytes[117..124].try_into()?,
  };
  let count = bytes[124] as usize;
  ensure!(
    count <= 24 && 125 + count * 42 == end,
    "Snapshot manifest entry count is invalid"
  );
  let mut entries = Vec::with_capacity(count);
  for bytes in bytes[125..end].as_chunks::<42>().0 {
    let slot = bytes[1];
    let record = match bytes[0] {
      1 => Record::Cpu(slot),
      2 if slot == 0 => Record::Software,
      3 if slot == 0 => Record::Uart,
      9 if slot == 0 => Record::Rtc,
      4 if slot == 0 => Record::Flash,
      5 if slot == 0 => Record::Interrupts,
      6 => Record::Memory(slot),
      7 => Record::Transport(slot),
      8 => Record::Backend(slot),
      _ => anyhow::bail!("Snapshot component kind/slot is unsupported"),
    };
    entries.push(Entry {
      record,
      length: u64::from_le_bytes(bytes[2..10].try_into()?),
      checksum: bytes[10..].try_into()?,
    });
  }
  Manifest::new(binding, found, entries)
}

pub(super) fn decode(bytes: &[u8], expected: Binding, layout: Layout) -> anyhow::Result<Manifest> {
  let manifest = inspect(bytes)?;
  ensure!(
    manifest.binding == expected,
    "Snapshot manifest identity/configuration mismatch"
  );
  ensure!(
    manifest.layout == layout,
    "Snapshot manifest topology mismatch"
  );
  Ok(manifest)
}
