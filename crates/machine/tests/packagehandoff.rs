#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use machine::snapshot::{
  manifest::{Binding, Layout},
  package::{Package, Receipt, Sealed, Stage},
};
use sha2::{Digest, Sha256};
use std::{
  fs::{File, Permissions},
  os::unix::fs::PermissionsExt,
  path::Path,
  process::Command,
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

fn owner(path: &Path) -> File {
  std::fs::set_permissions(path, Permissions::from_mode(0o700)).unwrap();
  File::open(path).unwrap()
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

fn name(receipt: Receipt) -> String {
  format!(
    ".stage{}",
    receipt
      .nonce
      .iter()
      .map(|byte| format!("{byte:02x}"))
      .collect::<String>()
  )
}

#[test]
fn writer_exits_before_host_reopens_and_commits() {
  let root = tempfile::tempdir().unwrap();
  let parent = owner(root.path());
  assert!(Command::new(std::env::current_exe().unwrap())
    .args(["--exact", "staging_process", "--ignored"])
    .env("HOPPER_HANDOFF_ROOT", root.path())
    .status()
    .unwrap()
    .success());
  assert!(Package::open(&parent, binding(), layout(), || Ok(())).is_err());
  let receipt =
    serde_json::from_slice(&std::fs::read(root.path().join("handoff.json")).unwrap()).unwrap();
  let first = Sealed::open(&parent, receipt, binding(), layout(), || Ok(())).unwrap();
  let duplicate = Sealed::open(&parent, receipt, binding(), layout(), || Ok(())).unwrap();
  drop(first.publish(|| Ok(()), || Ok(())).unwrap());
  assert!(duplicate.publish(|| Ok(()), || Ok(())).is_err());
  assert!(Sealed::open(&parent, receipt, binding(), layout(), || Ok(())).is_err());
  assert!(Package::open(&parent, binding(), layout(), || Ok(())).is_ok());
  assert!(!root.path().join(name(receipt)).exists());
}

#[test]
#[ignore = "invoked in a separate process"]
fn staging_process() {
  let path = std::env::var_os("HOPPER_HANDOFF_ROOT").unwrap();
  let parent = owner(Path::new(&path));
  let sealed = stage(&parent).seal(|| Ok(())).unwrap();
  std::fs::write(
    Path::new(&path).join("handoff.json"),
    serde_json::to_vec(&sealed.receipt()).unwrap(),
  )
  .unwrap();
  drop(sealed);
}

#[test]
fn receipts_cannot_select_another_stage_or_configuration() {
  let root = tempfile::tempdir().unwrap();
  let parent = owner(root.path());
  let sealed = stage(&parent).seal(|| Ok(())).unwrap();
  let receipt = sealed.receipt();
  drop(sealed);
  for fault in 0..7 {
    let mut supplied = receipt;
    let mut expected = binding();
    let mut geometry = layout();
    match fault {
      0 => supplied.nonce[0] ^= 1,
      1 => supplied.device ^= 1,
      2 => supplied.inode ^= 1,
      3 => supplied.manifest[0] ^= 1,
      4 => expected.machine = [4; 16],
      5 => expected.snapshot = [4; 16],
      _ => geometry.cpus = 2,
    }
    assert!(Sealed::open(&parent, supplied, expected, geometry, || Ok(())).is_err());
  }
  assert!(Sealed::open(&parent, receipt, binding(), layout(), || Ok(())).is_ok());
  assert!(Package::open(&parent, binding(), layout(), || Ok(())).is_err());
}

#[test]
fn stage_replacement_and_symlink_never_redirect_commit() {
  for symlink in [false, true] {
    let root = tempfile::tempdir().unwrap();
    let parent = owner(root.path());
    let sealed = stage(&parent).seal(|| Ok(())).unwrap();
    let receipt = sealed.receipt();
    let path = root.path().join(name(receipt));
    let old = root.path().join("preserved");
    std::fs::rename(&path, &old).unwrap();
    if symlink {
      std::os::unix::fs::symlink(&old, &path).unwrap();
    } else {
      std::fs::create_dir(&path).unwrap();
      owner(&path);
    }
    assert!(Sealed::open(&parent, receipt, binding(), layout(), || Ok(())).is_err());
    assert!(sealed.publish(|| Ok(()), || Ok(())).is_err());
    assert!(old.join("manifest.hsm").exists());
    assert!(!root.path().join("02".repeat(16)).exists());
  }
}

#[test]
fn corrupt_manifest_and_revoked_handoff_preserve_the_stage() {
  let root = tempfile::tempdir().unwrap();
  let parent = owner(root.path());
  let sealed = stage(&parent).seal(|| Ok(())).unwrap();
  let receipt = sealed.receipt();
  drop(sealed);
  assert!(
    Sealed::open(&parent, receipt, binding(), layout(), || anyhow::bail!(
      "revoked"
    ))
    .is_err()
  );
  let reopened = Sealed::open(&parent, receipt, binding(), layout(), || Ok(())).unwrap();
  assert!(reopened
    .publish(|| anyhow::bail!("revoked"), || Ok(()))
    .is_err());
  assert!(root.path().join(name(receipt)).exists());
  let cached = Sealed::open(&parent, receipt, binding(), layout(), || Ok(())).unwrap();
  let path = root.path().join(name(receipt)).join("manifest.hsm");
  let mut bytes = std::fs::read(&path).unwrap();
  bytes[0] ^= 1;
  std::fs::set_permissions(&path, Permissions::from_mode(0o600)).unwrap();
  std::fs::write(&path, bytes).unwrap();
  std::fs::set_permissions(&path, Permissions::from_mode(0o400)).unwrap();
  assert!(Sealed::open(&parent, receipt, binding(), layout(), || Ok(())).is_err());
  assert!(cached.publish(|| Ok(()), || Ok(())).is_err());
  assert!(Package::open(&parent, binding(), layout(), || Ok(())).is_err());
}

#[test]
fn revoked_sealing_never_returns_a_receipt_or_publishes() {
  for deny in 0..2 {
    let root = tempfile::tempdir().unwrap();
    let parent = owner(root.path());
    let checks = std::cell::Cell::new(0);
    let result = stage(&parent).seal(|| {
      let current = checks.get();
      checks.set(current + 1);
      anyhow::ensure!(current != deny, "revoked");
      Ok(())
    });
    assert!(result.is_err());
    assert!(Package::open(&parent, binding(), layout(), || Ok(())).is_err());
    let entries: Vec<_> = std::fs::read_dir(root.path())
      .unwrap()
      .map(|entry| entry.unwrap())
      .collect();
    assert_eq!(entries.len(), 1);
    assert!(entries[0]
      .file_name()
      .to_string_lossy()
      .starts_with(".stage"));
    assert_eq!(entries[0].path().join("manifest.hsm").exists(), deny == 1);
  }
}
