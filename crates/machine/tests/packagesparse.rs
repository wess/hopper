#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use machine::snapshot::{
  manifest::{Binding, Layout, Record},
  package::{Package, Stage},
};
use sha2::{Digest, Sha256};
use std::{
  fs,
  io::Read,
  os::unix::fs::{MetadataExt, PermissionsExt},
};

#[test]
fn zero_chunk_components_reopen_with_identical_bytes_and_verified_checksums() {
  let root = tempfile::tempdir().unwrap();
  fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
  let parent = fs::File::open(root.path()).unwrap();
  let binding = Binding {
    machine: [1; 16],
    snapshot: [2; 16],
    configuration: [3; 32],
  };
  let layout = Layout {
    cpus: 1,
    banks: [(0, 65536), (65536, 65536), (1 << 30, 4 << 20)],
    devices: [0; 7],
  };
  let mut bytes = vec![0; 4 << 20];
  bytes[65535] = 0x51;
  bytes[3 << 20] = 0x72;
  bytes.extend_from_slice(&Sha256::digest(&bytes));
  let mut stage = Stage::new(&parent, binding, layout, || Ok(())).unwrap();
  for record in layout.records().unwrap() {
    let payload = if record == Record::Memory(2) {
      bytes.clone()
    } else {
      let mut payload = b"metadata fixture".to_vec();
      payload.extend_from_slice(&Sha256::digest(&payload));
      payload
    };
    stage
      .write(
        record,
        || Ok(()),
        |out| {
          for chunk in payload.chunks(16381) {
            out.write_all(chunk)?;
          }
          Ok(())
        },
      )
      .unwrap();
  }
  drop(stage.publish(|| Ok(()), || Ok(())).unwrap());
  let path = root.path().join("02".repeat(16)).join("ram.hrm");
  let metadata = fs::metadata(&path).unwrap();
  assert_eq!(metadata.len(), bytes.len() as u64);
  assert_eq!(metadata.mode() & 0o7777, 0o400);
  let package = Package::open(&parent, binding, layout, || Ok(())).unwrap();
  let mut reader = package.reader(Record::Memory(2), || Ok(())).unwrap();
  let mut reopened = Vec::new();
  reader.read_to_end(&mut reopened).unwrap();
  reader.finish().unwrap();
  assert_eq!(reopened, bytes);
}
