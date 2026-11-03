#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use machine::snapshot::manifest::{Binding, Entry, Layout, Manifest, Record, MAX};
use sha2::{Digest, Sha256};

#[path = "manifest/stream.rs"]
mod stream;

fn binding() -> Binding {
  Binding {
    machine: [1; 16],
    snapshot: [2; 16],
    configuration: [3; 32],
  }
}

fn layout() -> Layout {
  Layout {
    cpus: 2,
    banks: [(0, 64 << 20), (64 << 20, 64 << 20), (1 << 30, 256 << 20)],
    devices: [1, 2, 3, 4, 5, 3, 4],
  }
}

fn entries(layout: Layout) -> Vec<Entry> {
  layout
    .records()
    .unwrap()
    .into_iter()
    .map(|record| Entry {
      record,
      length: 40,
      checksum: [5; 32],
    })
    .collect()
}

fn saved() -> Manifest {
  Manifest::new(binding(), layout(), entries(layout())).unwrap()
}

fn checksum(bytes: &mut [u8]) {
  let end = bytes.len() - 32;
  let checksum = Sha256::digest(&bytes[..end]);
  bytes[end..].copy_from_slice(&checksum);
}

#[test]
fn maximum_complete_inventory_round_trips_and_binds_every_component() {
  let bytes = saved().encode();
  assert_eq!(bytes.len(), 1165);
  assert!(bytes.len() <= MAX);
  let saved = Manifest::decode(&bytes, binding(), layout()).unwrap();
  assert_eq!(saved.encode(), bytes);
  assert_eq!(layout().records().unwrap().len(), 24);
  for record in layout().records().unwrap() {
    saved.check(record, 40, [5; 32]).unwrap();
    assert!(saved.check(record, 41, [5; 32]).is_err());
    assert!(saved.check(record, 40, [6; 32]).is_err());
  }
  assert!(saved.check(Record::Cpu(2), 40, [5; 32]).is_err());
  assert!(saved.check(Record::Memory(3), 40, [5; 32]).is_err());
}

#[test]
fn incomplete_duplicate_foreign_and_oversized_records_reject() {
  let topology = layout();
  let mut records = entries(topology);
  records.pop();
  assert!(Manifest::new(binding(), topology, records).is_err());
  let mut records = entries(topology);
  records[1].record = records[0].record;
  assert!(Manifest::new(binding(), topology, records).is_err());
  let mut records = entries(topology);
  records.swap(0, 1);
  assert!(Manifest::new(binding(), topology, records).is_err());
  for record in topology.records().unwrap() {
    let mut records = entries(topology);
    records
      .iter_mut()
      .find(|entry| entry.record == record)
      .unwrap()
      .length = topology.limit(record).unwrap() + 1;
    assert!(Manifest::new(binding(), topology, records).is_err());
  }
  let mut no_devices = topology;
  no_devices.devices = [0; 7];
  assert!(Manifest::new(binding(), no_devices, entries(topology)).is_err());
  let mut invalid = topology;
  invalid.banks[1] = invalid.banks[0];
  assert!(invalid.records().is_err());
  invalid = topology;
  invalid.banks[2].0 = u64::MAX;
  assert!(invalid.records().is_err());
  invalid = topology;
  invalid.devices[6] = 6;
  assert!(invalid.records().is_err());
}

#[test]
fn every_truncation_damage_and_rechecksummed_invalid_metadata_rejects() {
  let bytes = saved().encode();
  for end in 0..bytes.len() {
    assert!(Manifest::decode(&bytes[..end], binding(), layout()).is_err());
  }
  for offset in 0..bytes.len() {
    let mut changed = bytes.clone();
    changed[offset] ^= 1;
    assert!(Manifest::decode(&changed, binding(), layout()).is_err());
  }
  for offset in [4, 20, 36, 68, 69, 77, 117, 124, 125, 126, 167, 168] {
    let mut changed = bytes.clone();
    changed[offset] ^= 8;
    checksum(&mut changed);
    assert!(Manifest::decode(&changed, binding(), layout()).is_err());
  }
  let mut extended = bytes.clone();
  extended.insert(extended.len() - 32, 0);
  checksum(&mut extended);
  assert!(Manifest::decode(&extended, binding(), layout()).is_err());
  let mut foreign = binding();
  foreign.machine[0] ^= 1;
  assert!(Manifest::decode(&bytes, foreign, layout()).is_err());
  let mut foreign = layout();
  foreign.devices[0] = 0;
  assert!(Manifest::decode(&bytes, binding(), foreign).is_err());
}

#[test]
fn previous_inventory_without_a_clock_is_not_silently_reset_on_restore() {
  let mut previous = saved().encode();
  let slot = layout()
    .records()
    .unwrap()
    .iter()
    .position(|record| *record == Record::Rtc)
    .unwrap();
  previous.drain(125 + slot * 42..125 + (slot + 1) * 42);
  previous[124] -= 1;
  previous[..4].copy_from_slice(b"HSM1");
  checksum(&mut previous);
  assert_eq!(previous.len(), 1123);
  let original = previous.clone();
  let error = Manifest::decode(&previous, binding(), layout())
    .err()
    .unwrap();
  assert!(error.to_string().contains("predates native clock state"));
  assert_eq!(previous, original);
}
