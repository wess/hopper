#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
use machine::snapshot::{
  manifest::{Binding, Layout, Record},
  package::{Package, Stage},
};
use sha2::{Digest, Sha256};
use std::{fs, os::unix::fs::PermissionsExt};

fn fixture() -> (tempfile::TempDir, Package) {
  let root = tempfile::tempdir().unwrap();
  fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
  let layout = Layout {
    cpus: 2,
    banks: [(0, 262144), (262144, 262144), (1 << 30, 262144)],
    devices: [0; 7],
  };
  let binding = Binding {
    machine: [1; 16],
    snapshot: [2; 16],
    configuration: [3; 32],
  };
  let mut stage = Stage::new(
    &fs::File::open(root.path()).unwrap(),
    binding,
    layout,
    || Ok(()),
  )
  .unwrap();
  for record in layout.records().unwrap() {
    let mut bytes = vec![
      0x51;
      if record == Record::Memory(2) {
        131079
      } else {
        11
      }
    ];
    bytes.extend_from_slice(&Sha256::digest(&bytes));
    stage
      .write(record, || Ok(()), |out| Ok(out.write_all(&bytes)?))
      .unwrap();
  }
  let package = stage.publish(|| Ok(()), || Ok(())).unwrap();
  (root, package)
}

#[test]
fn complete_bounded_streams_reject_changed_bodies_without_relying_on_changed_footers() {
  let (root, package) = fixture();
  package.verify_components(|| Ok(())).unwrap();
  for record in package.layout().records().unwrap() {
    let path = root.path().join("02".repeat(16)).join(record.name());
    let original = fs::read(&path).unwrap();
    let mut damaged = original.clone();
    damaged[0] ^= 1;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(&path, damaged).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
    assert!(
      package.verify_components(|| Ok(())).is_err(),
      "{}",
      record.name()
    );
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(&path, original).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o400)).unwrap();
  }
  package.verify_components(|| Ok(())).unwrap();
}

#[test]
fn truncation_trailing_bytes_and_nonprivate_components_reject() {
  for fault in 0..3 {
    let (root, package) = fixture();
    let path = root.path().join("02".repeat(16)).join("ram.hrm");
    let mut bytes = fs::read(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    if fault == 0 {
      bytes.pop();
    }
    if fault == 1 {
      bytes.push(0);
    }
    fs::write(&path, bytes).unwrap();
    if fault != 2 {
      fs::set_permissions(path, fs::Permissions::from_mode(0o400)).unwrap();
    }
    assert!(package.verify_components(|| Ok(())).is_err());
  }
}

#[test]
fn revocation_at_every_observed_checkpoint_stops_integrity_verification() {
  let (_root, package) = fixture();
  let mut count = 0;
  package
    .verify_components(|| {
      count += 1;
      Ok(())
    })
    .unwrap();
  for deny in 1..=count {
    let mut observed = 0;
    assert!(package
      .verify_components(|| {
        observed += 1;
        anyhow::ensure!(observed != deny, "revoked");
        Ok(())
      })
      .is_err());
    assert_eq!(observed, deny);
  }
  package.verify_components(|| Ok(())).unwrap();
}
