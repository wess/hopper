use machine::{
  runtime::variables,
  snapshot::{
    manifest::{Binding, Entry, Layout, Manifest},
    selection::{Phase, Record},
  },
};
use sha2::{Digest, Sha256};
use std::{
  fs::{self, File, OpenOptions, Permissions},
  os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
  path::PathBuf,
};
pub struct Fixture {
  pub root: tempfile::TempDir,
  pub generation: PathBuf,
  pub record: Record,
}
pub fn private(path: &std::path::Path) {
  fs::create_dir(path).unwrap();
  fs::set_permissions(path, Permissions::from_mode(0o700)).unwrap();
}
pub fn sealed(path: &std::path::Path, bytes: &[u8]) {
  if path.exists() {
    fs::set_permissions(path, Permissions::from_mode(0o600)).unwrap();
  }
  fs::write(path, bytes).unwrap();
  fs::set_permissions(path, Permissions::from_mode(0o400)).unwrap();
}
impl Fixture {
  pub fn new() -> Self {
    let root = tempfile::tempdir().unwrap();
    fs::set_permissions(root.path(), Permissions::from_mode(0o700)).unwrap();
    let record = Record {
      machine: [1; 16],
      generation: [4; 16],
      snapshot: [2; 16],
      configuration: [3; 32],
      rollback: [5; 16],
      previous: None,
      phase: Phase::Active,
    };
    private(&root.path().join("generations"));
    let generation = root.path().join("generations").join("04".repeat(16));
    private(&generation);
    let layout = Layout {
      cpus: 1,
      banks: [(0, 65536), (65536, 65536), (1 << 30, 65536)],
      devices: [1, 0, 0, 0, 0, 0, 0],
    };
    // provenance metadata only; absent source payloads cannot be used to restore a CPU checkpoint.
    let entries = layout
      .records()
      .unwrap()
      .into_iter()
      .map(|record| Entry {
        record,
        length: 36,
        checksum: [8; 32],
      })
      .collect();
    let manifest = Manifest::new(
      Binding {
        machine: record.machine,
        snapshot: record.snapshot,
        configuration: record.configuration,
      },
      layout,
      entries,
    )
    .unwrap();
    let mut receipt = Vec::from(b"HGR1");
    receipt.extend_from_slice(&record.generation);
    receipt.extend_from_slice(&manifest.encode());
    receipt.extend_from_slice(&Sha256::digest(&receipt));
    sealed(&generation.join("generation.hgr"), &receipt);
    let mut marker = Vec::from(b"HGA1");
    marker.extend_from_slice(&Sha256::digest(&receipt));
    marker.extend_from_slice(&[7; 16]);
    marker.extend_from_slice(&Sha256::digest(&marker));
    sealed(&generation.join("attempt.hga"), &marker);
    private(&generation.join("storage0"));
    let disk = OpenOptions::new()
      .write(true)
      .create_new(true)
      .mode(0o600)
      .open(generation.join("storage0/disk"))
      .unwrap();
    disk.set_len(4096).unwrap();
    drop(variables::create(&generation.join("variables"), &[0xff]).unwrap());
    let mut ownership = Vec::from(b"HGO1");
    ownership.extend_from_slice(&Sha256::digest(&receipt));
    for path in [&generation, &generation.join("variables")] {
      let metadata = fs::metadata(path).unwrap();
      ownership.extend_from_slice(&metadata.dev().to_le_bytes());
      ownership.extend_from_slice(&metadata.ino().to_le_bytes());
    }
    for slot in 0..7 {
      if slot == 0 {
        ownership.push(1);
        for path in [
          generation.join("storage0"),
          generation.join("storage0/disk"),
        ] {
          let metadata = fs::metadata(path).unwrap();
          ownership.extend_from_slice(&metadata.dev().to_le_bytes());
          ownership.extend_from_slice(&metadata.ino().to_le_bytes());
        }
        ownership.extend_from_slice(&4096u64.to_le_bytes());
      } else {
        ownership.extend_from_slice(&[0; 41]);
      }
    }
    ownership.extend_from_slice(&Sha256::digest(&ownership));
    sealed(&generation.join("ownership.hgo"), &ownership);
    sealed(&root.path().join("restore.hrr"), &record.encode().unwrap());
    Self {
      root,
      generation,
      record,
    }
  }
  pub fn open(&self) -> File {
    File::open(self.root.path()).unwrap()
  }
}
