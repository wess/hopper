#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use machine::snapshot::{
  manifest::{Binding, Layout},
  package::{inventory, Description, Stage},
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
fn named_package_round_trips_without_a_separate_index() {
  let (_root, parent) = fixture();
  let mut sealed = stage(&parent).seal(|| Ok(())).unwrap();
  sealed
    .describe(
      &Description::new("  Before updates 🛠  ", 1791822930).unwrap(),
      || Ok(()),
    )
    .unwrap();
  assert!(inventory(&parent, [1; 16], || Ok(())).unwrap().is_empty());
  drop(sealed.publish(|| Ok(()), || Ok(())).unwrap());
  let entries = inventory(&parent, [1; 16], || Ok(())).unwrap();
  let description = entries[0].description.as_ref().unwrap();
  assert_eq!(description.name(), "Before updates 🛠");
  assert_eq!(description.created(), 1791822930);
}

#[test]
fn invalid_names_and_dates_reject_before_writing() {
  for name in [
    "",
    "  ",
    "line\nbreak",
    "nul\0byte",
    "\t",
    &"x".repeat(101),
    &"🛠".repeat(26),
  ] {
    assert!(Description::new(name, 1).is_err());
  }
  assert!(Description::new(&"x".repeat(100), 1).is_ok());
  assert!(Description::new("valid", 0).is_err());
  assert!(Description::new("valid", u64::MAX).is_err());
}

#[test]
fn changed_or_removed_metadata_rejects_commit_without_publishing() {
  for remove in [false, true] {
    let (root, parent) = fixture();
    let mut sealed = stage(&parent).seal(|| Ok(())).unwrap();
    sealed
      .describe(&Description::new("Original", 1).unwrap(), || Ok(()))
      .unwrap();
    let path = std::fs::read_dir(root.path())
      .unwrap()
      .next()
      .unwrap()
      .unwrap()
      .path()
      .join("description.hsn");
    if remove {
      std::fs::remove_file(&path).unwrap();
    } else {
      std::fs::set_permissions(&path, Permissions::from_mode(0o600)).unwrap();
      std::fs::write(&path, b"corrupt").unwrap();
      std::fs::set_permissions(&path, Permissions::from_mode(0o400)).unwrap();
    }
    assert!(sealed.publish(|| Ok(()), || Ok(())).is_err());
    assert!(!root.path().join("02".repeat(16)).exists());
    assert!(std::fs::read_dir(root.path()).unwrap().next().is_some());
  }
}

#[test]
fn copied_foreign_description_and_symlink_never_supply_a_name() {
  let (root, parent) = fixture();
  let mut sealed = stage(&parent).seal(|| Ok(())).unwrap();
  sealed
    .describe(&Description::new("Original", 1).unwrap(), || Ok(()))
    .unwrap();
  drop(sealed.publish(|| Ok(()), || Ok(())).unwrap());
  let path = root.path().join("02".repeat(16)).join("description.hsn");
  let original = std::fs::read(&path).unwrap();
  for offset in 0..original.len() {
    let mut changed = original.clone();
    changed[offset] ^= 1;
    std::fs::set_permissions(&path, Permissions::from_mode(0o600)).unwrap();
    std::fs::write(&path, changed).unwrap();
    std::fs::set_permissions(&path, Permissions::from_mode(0o400)).unwrap();
    assert!(inventory(&parent, [1; 16], || Ok(())).is_err());
  }
  for offset in [4, 20, 36, 68, 76, 77] {
    let mut foreign = original.clone();
    if offset == 68 {
      foreign[68..76].fill(0);
    } else {
      foreign[offset] ^= 0xff;
    }
    let end = foreign.len() - 32;
    let digest = Sha256::digest(&foreign[..end]);
    foreign[end..].copy_from_slice(&digest);
    std::fs::set_permissions(&path, Permissions::from_mode(0o600)).unwrap();
    std::fs::write(&path, foreign).unwrap();
    std::fs::set_permissions(&path, Permissions::from_mode(0o400)).unwrap();
    assert!(inventory(&parent, [1; 16], || Ok(())).is_err());
  }
  let old = path.with_extension("preserved");
  std::fs::rename(&path, &old).unwrap();
  std::os::unix::fs::symlink(&old, &path).unwrap();
  assert!(inventory(&parent, [1; 16], || Ok(())).is_err());
}

#[test]
fn cancellation_during_description_preserves_private_stage() {
  let (root, parent) = fixture();
  let mut sealed = stage(&parent).seal(|| Ok(())).unwrap();
  let mut checks = 0;
  assert!(sealed
    .describe(&Description::new("Original", 1).unwrap(), || {
      checks += 1;
      anyhow::ensure!(checks < 2, "cancelled");
      Ok(())
    })
    .is_err());
  assert!(!root.path().join("02".repeat(16)).exists());
  assert!(inventory(&parent, [1; 16], || Ok(())).unwrap().is_empty());
}
