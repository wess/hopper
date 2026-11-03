//! HSM2 binds complete component inventories; component decoders verify payload checksums.

use crate::{
  devices::{
    flash, rtc, serial,
    virtio::{block, console, gpu, input, pci, scsi},
  },
  hypervisor::{gic, state::group},
};
use anyhow::ensure;
mod codec;
mod stream;
pub use stream::Checked;
pub const MAX: usize = 1200;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Binding {
  pub machine: [u8; 16],
  pub snapshot: [u8; 16],
  pub configuration: [u8; 32],
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Layout {
  pub cpus: u8,
  pub banks: [(u64, u64); 3],
  pub devices: [u8; 7],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Record {
  Cpu(u8),
  Software,
  Uart,
  Rtc,
  Flash,
  Interrupts,
  Memory(u8),
  Transport(u8),
  Backend(u8),
}

pub struct Entry {
  pub record: Record,
  pub length: u64,
  /// The SHA-256 footer already verified by each component decoder, not a file hash.
  pub checksum: [u8; 32],
}

pub struct Manifest {
  binding: Binding,
  layout: Layout,
  entries: Vec<Entry>,
}

impl Record {
  pub fn name(self) -> String {
    match self {
      Self::Cpu(slot) => format!("{slot}.hcp"),
      Self::Software => "software.hsw".into(),
      Self::Uart => "uart.hur".into(),
      Self::Rtc => "clock.hrt".into(),
      Self::Flash => "flash.hfl".into(),
      Self::Interrupts => "interrupts.hgi".into(),
      Self::Memory(slot) => match slot {
        0 => "firmware.hrm".into(),
        1 => "nvram.hrm".into(),
        _ => "ram.hrm".into(),
      },
      Self::Transport(slot) => format!("transport{slot}.hvt"),
      Self::Backend(slot) => format!("backend{slot}.hvb"),
    }
  }
}

impl Layout {
  pub fn records(&self) -> anyhow::Result<Vec<Record>> {
    ensure!(
      (1..=2).contains(&self.cpus) && self.devices.iter().all(|kind| *kind <= 5),
      "Snapshot topology is unsupported"
    );
    for (index, &(address, size)) in self.banks.iter().enumerate() {
      ensure!(
        (4096..=64 << 30).contains(&size)
          && size.is_multiple_of(4096)
          && address.is_multiple_of(4096)
          && address.checked_add(size).is_some_and(|end| end <= 1 << 40),
        "Snapshot memory geometry is invalid"
      );
      for &(other, length) in &self.banks[..index] {
        ensure!(
          address + size <= other || other + length <= address,
          "Snapshot memory banks overlap"
        );
      }
    }
    let mut records = (0..self.cpus).map(Record::Cpu).collect::<Vec<_>>();
    records.extend([
      Record::Software,
      Record::Uart,
      Record::Rtc,
      Record::Flash,
      Record::Interrupts,
    ]);
    records.extend((0..3).map(Record::Memory));
    for constructor in [Record::Transport, Record::Backend] {
      records.extend(
        self
          .devices
          .iter()
          .enumerate()
          .filter(|(_, kind)| **kind != 0)
          .map(|(slot, _)| constructor(slot as u8)),
      );
    }
    Ok(records)
  }

  pub fn limit(&self, record: Record) -> anyhow::Result<u64> {
    ensure!(
      self.records()?.contains(&record),
      "Record is outside snapshot topology"
    );
    Ok(match record {
      Record::Cpu(_) => group::wire::MAX as u64,
      Record::Software => super::wire::MAX as u64,
      Record::Uart => serial::wire::MAX as u64,
      Record::Rtc => rtc::wire::SIZE as u64,
      Record::Flash => flash::snapshot::wire::MAX as u64,
      Record::Interrupts => gic::state::wire::MAX as u64,
      Record::Memory(slot) => {
        let size = self.banks[slot as usize].1;
        size + size.div_ceil(65536) * 4 + 56
      }
      Record::Transport(_) => pci::snapshot::wire::MAX as u64,
      Record::Backend(slot) => match self.devices[slot as usize] {
        1 => block::snapshot::record::SIZE as u64,
        2 => gpu::snapshot::wire::MAX,
        3 => input::wire::MAX as u64,
        4 => scsi::snapshot::record::SIZE as u64,
        5 => console::wire::MAX as u64,
        _ => unreachable!(),
      },
    })
  }
}

impl Manifest {
  /// Discovery only. Restore must supply independently authorized binding and hardware to decode.
  pub fn inspect(bytes: &[u8]) -> anyhow::Result<Self> {
    codec::inspect(bytes)
  }

  pub fn binding(&self) -> Binding {
    self.binding
  }

  pub fn layout(&self) -> Layout {
    self.layout
  }

  pub fn new(binding: Binding, layout: Layout, entries: Vec<Entry>) -> anyhow::Result<Self> {
    ensure!(
      binding.machine != [0; 16] && binding.snapshot != [0; 16] && binding.configuration != [0; 32],
      "Snapshot binding is unset"
    );
    let required = layout.records()?;
    ensure!(
      entries.len() == required.len() && entries.iter().map(|entry| entry.record).eq(required),
      "Snapshot inventory is incomplete, duplicated or unordered"
    );
    for entry in &entries {
      ensure!(
        (36..=layout.limit(entry.record)?).contains(&entry.length),
        "Snapshot component exceeds its bound"
      );
    }
    Ok(Self {
      binding,
      layout,
      entries,
    })
  }

  pub fn encode(&self) -> Vec<u8> {
    codec::encode(self)
  }

  pub fn decode(bytes: &[u8], expected: Binding, layout: Layout) -> anyhow::Result<Self> {
    codec::decode(bytes, expected, layout)
  }

  /// Footer admission alone is insufficient: decode every component before using the checkpoint.
  pub fn check(&self, record: Record, length: u64, checksum: [u8; 32]) -> anyhow::Result<()> {
    let entry = self
      .entries
      .iter()
      .find(|entry| entry.record == record)
      .ok_or_else(|| anyhow::anyhow!("Component is absent from snapshot manifest"))?;
    ensure!(
      entry.length == length && entry.checksum == checksum,
      "Snapshot component does not match its manifest"
    );
    Ok(())
  }

  pub fn reader<R: std::io::Read, A: FnMut() -> anyhow::Result<()>>(
    &self,
    record: Record,
    input: R,
    authorize: A,
  ) -> anyhow::Result<Checked<'_, R, A>> {
    let entry = self
      .entries
      .iter()
      .find(|entry| entry.record == record)
      .ok_or_else(|| anyhow::anyhow!("Component is absent from snapshot manifest"))?;
    Ok(Checked::new(entry, input, authorize))
  }
}
