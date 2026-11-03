use super::*;
use machine::devices::serial;
use std::io::{self, Read};

fn fixture() -> (Manifest, Vec<u8>) {
  let bytes = serial::wire::encode(&serial::Console::default())
    .unwrap()
    .to_vec();
  let mut entries = entries(layout());
  let entry = entries
    .iter_mut()
    .find(|entry| entry.record == Record::Uart)
    .unwrap();
  entry.length = bytes.len() as u64;
  entry.checksum.copy_from_slice(&bytes[bytes.len() - 32..]);
  (Manifest::new(binding(), layout(), entries).unwrap(), bytes)
}

struct Fragments<'a>(&'a [u8], usize);
impl Read for Fragments<'_> {
  fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
    assert!(output.len() <= 65536);
    let count = output.len().min(self.1);
    self.0.read(&mut output[..count])
  }
}

#[test]
fn fragmented_consumed_footer_matches_and_foreign_valid_component_rejects() {
  let (manifest, bytes) = fixture();
  for count in [1, 7, 31, 32, 33, 65536] {
    let mut reader = manifest
      .reader(Record::Uart, Fragments(&bytes, count), || Ok(()))
      .unwrap();
    let mut consumed = Vec::new();
    reader.read_to_end(&mut consumed).unwrap();
    serial::wire::decode(&consumed).unwrap();
    reader.finish().unwrap();
    assert_eq!(consumed, bytes);
  }
  let mut console = serial::Console::default();
  console.control = 0x301;
  let foreign = serial::wire::encode(&console).unwrap();
  let mut reader = manifest
    .reader(Record::Uart, foreign.as_slice(), || Ok(()))
    .unwrap();
  let mut consumed = Vec::new();
  reader.read_to_end(&mut consumed).unwrap();
  serial::wire::decode(&consumed).unwrap();
  assert!(reader.finish().is_err());
}

#[test]
fn incomplete_extra_and_revoked_reads_cannot_be_admitted() {
  let (manifest, bytes) = fixture();
  let mut reader = manifest
    .reader(Record::Uart, &bytes[..bytes.len() - 1], || Ok(()))
    .unwrap();
  reader.read_to_end(&mut Vec::new()).unwrap();
  assert!(reader.finish().is_err());
  let mut extra = bytes.clone();
  extra.push(0);
  let mut reader = manifest
    .reader(Record::Uart, extra.as_slice(), || Ok(()))
    .unwrap();
  assert!(reader.read_to_end(&mut Vec::new()).is_err());
  assert!(reader.finish().is_err());
  let mut checks = 0;
  let mut reader = manifest
    .reader(Record::Uart, bytes.as_slice(), || {
      checks += 1;
      Ok(())
    })
    .unwrap();
  reader.read_to_end(&mut Vec::new()).unwrap();
  reader.finish().unwrap();
  for stop in 1..=checks {
    let mut calls = 0;
    let mut reader = manifest
      .reader(Record::Uart, bytes.as_slice(), || {
        calls += 1;
        anyhow::ensure!(calls < stop, "revoked");
        Ok(())
      })
      .unwrap();
    let read = reader.read_to_end(&mut Vec::new());
    assert!(read.is_err() || reader.finish().is_err());
  }
}

#[test]
fn footer_admission_does_not_replace_component_payload_verification() {
  let (manifest, mut bytes) = fixture();
  bytes[4] ^= 1;
  let mut reader = manifest
    .reader(Record::Uart, bytes.as_slice(), || Ok(()))
    .unwrap();
  let mut consumed = Vec::new();
  reader.read_to_end(&mut consumed).unwrap();
  assert!(serial::wire::decode(&consumed).is_err());
  // a matching footer is only the manifest commitment; payload integrity needs decode.
  reader.finish().unwrap();
}

#[test]
fn revocation_after_read_clears_returned_bytes_and_large_reads_stay_bounded() {
  let (manifest, bytes) = fixture();
  let mut calls = 0;
  let mut reader = manifest
    .reader(Record::Uart, bytes.as_slice(), || {
      calls += 1;
      anyhow::ensure!(calls < 2, "revoked");
      Ok(())
    })
    .unwrap();
  let mut output = vec![0xaa; 131072];
  assert!(reader.read(&mut output).is_err());
  assert!(output[..bytes.len()].iter().all(|byte| *byte == 0));
  assert!(reader.finish().is_err());

  let mut data = vec![7; 131072];
  let checksum: [u8; 32] = Sha256::digest(&data).into();
  data.extend_from_slice(&checksum);
  let mut entries = entries(layout());
  let entry = entries
    .iter_mut()
    .find(|entry| entry.record == Record::Backend(1))
    .unwrap();
  entry.length = data.len() as u64;
  entry.checksum = checksum;
  let manifest = Manifest::new(binding(), layout(), entries).unwrap();
  let mut reader = manifest
    .reader(Record::Backend(1), Fragments(&data, usize::MAX), || Ok(()))
    .unwrap();
  let mut consumed = 0;
  loop {
    let count = reader.read(&mut output).unwrap();
    if count == 0 {
      break;
    }
    assert!(count <= 65536);
    consumed += count;
  }
  assert_eq!(consumed, data.len());
  reader.finish().unwrap();
}
