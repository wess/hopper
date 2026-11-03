#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use machine::snapshot::{
  manifest::{Binding, Layout},
  package::{inventory, Stage},
};
use sha2::{Digest, Sha256};
use std::{
  fs::{File, Permissions},
  os::unix::fs::PermissionsExt,
};

fn binding() -> Binding {
  Binding {
    machine: [1; 16],
    snapshot: [2; 16],
    configuration: [3; 32],
  }
}

fn layout() -> Layout {
  Layout {
    cpus: 1,
    banks: [(0, 65536), (65536, 65536), (1 << 30, 65536)],
    devices: [0; 7],
  }
}

fn fixture() -> (tempfile::TempDir, File) {
  let root = tempfile::tempdir().unwrap();
  std::fs::set_permissions(root.path(), Permissions::from_mode(0o700)).unwrap();
  let parent = File::open(root.path()).unwrap();
  (root, parent)
}

fn stage(parent: &File) -> Stage {
  let mut stage = Stage::new(parent, binding(), layout(), || Ok(())).unwrap();
  let mut bytes = b"test component".to_vec();
  bytes.extend_from_slice(&Sha256::digest(&bytes));
  for record in layout().records().unwrap() {
    stage
      .write(record, || Ok(()), |out| Ok(out.write_all(&bytes)?))
      .unwrap();
  }
  stage
}

#[test]
fn only_published_manifests_are_discovered_with_independent_listing_cursors() {
  let (root, parent) = fixture();
  drop(stage(&parent).seal(|| Ok(())).unwrap());
  std::fs::write(root.path().join("unknown"), b"preserve").unwrap();
  assert!(inventory(&parent, [1; 16], || Ok(())).unwrap().is_empty());
  drop(stage(&parent).publish(|| Ok(()), || Ok(())).unwrap());
  for _ in 0..2 {
    let entries = inventory(&parent, [1; 16], || Ok(())).unwrap();
    assert_eq!(entries.len(), 1);
    assert!(entries[0].binding == binding());
    assert!(entries[0].layout == layout());
  }
  assert_eq!(
    std::fs::read(root.path().join("unknown")).unwrap(),
    b"preserve"
  );
  assert!(inventory(&parent, [4; 16], || Ok(())).is_err());
}

#[test]
fn renamed_foreign_corrupt_and_symlink_packages_are_never_hidden_or_followed() {
  for fault in 0..4 {
    let (root, parent) = fixture();
    drop(stage(&parent).publish(|| Ok(()), || Ok(())).unwrap());
    let path = root.path().join("02".repeat(16));
    match fault {
      0 => std::fs::rename(&path, root.path().join("04".repeat(16))).unwrap(),
      1 => {
        let old = root.path().join("preserved");
        std::fs::rename(&path, &old).unwrap();
        std::os::unix::fs::symlink(&old, &path).unwrap();
      }
      2 => {
        let manifest = path.join("manifest.hsm");
        std::fs::set_permissions(&manifest, Permissions::from_mode(0o600)).unwrap();
        std::fs::write(&manifest, b"damaged").unwrap();
        std::fs::set_permissions(&manifest, Permissions::from_mode(0o400)).unwrap();
      }
      _ => {
        let manifest = path.join("manifest.hsm");
        let old = path.join("preserved");
        std::fs::rename(&manifest, &old).unwrap();
        std::os::unix::fs::symlink(&old, &manifest).unwrap();
      }
    }
    assert!(inventory(&parent, [1; 16], || Ok(())).is_err());
    assert!(std::fs::read_dir(root.path()).unwrap().next().is_some());
  }
}

#[test]
fn revocation_during_listing_returns_no_partial_inventory() {
  let (_root, parent) = fixture();
  drop(stage(&parent).publish(|| Ok(()), || Ok(())).unwrap());
  let mut checks = 0;
  assert!(inventory(&parent, [1; 16], || {
    checks += 1;
    anyhow::ensure!(checks < 4, "revoked");
    Ok(())
  })
  .is_err());
  assert!(inventory(&parent, [1; 16], || Ok(())).is_ok());
}

#[test]
fn listing_is_bounded_even_when_every_entry_is_unknown() {
  let (root, parent) = fixture();
  for index in 0..4096 {
    File::create(root.path().join(format!("unknown{index}"))).unwrap();
  }
  assert!(inventory(&parent, [1; 16], || Ok(())).is_err());
  assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 4096);
}

#[test]
fn nonprivate_or_hardlinked_manifests_reject() {
  for fault in 0..3 {
    let (root, parent) = fixture();
    drop(stage(&parent).publish(|| Ok(()), || Ok(())).unwrap());
    let path = root.path().join("02".repeat(16));
    match fault {
      0 => std::fs::set_permissions(root.path(), Permissions::from_mode(0o755)).unwrap(),
      1 => std::fs::set_permissions(&path, Permissions::from_mode(0o755)).unwrap(),
      _ => std::fs::hard_link(path.join("manifest.hsm"), root.path().join("linked")).unwrap(),
    }
    assert!(inventory(&parent, [1; 16], || Ok(())).is_err());
  }
}
