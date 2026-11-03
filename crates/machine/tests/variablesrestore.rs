#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
use machine::{
  devices::flash,
  hypervisor::ram,
  runtime::variables::{self, SIZE},
};
use sha2::{Digest, Sha256};

fn memory(guest: u64, size: usize) -> ram::Capture {
  let mut bytes = Vec::from(b"HRM1");
  bytes.extend_from_slice(&guest.to_le_bytes());
  bytes.extend_from_slice(&(size as u64).to_le_bytes());
  bytes.extend_from_slice(&65536u32.to_le_bytes());
  for _ in 0..size.div_ceil(65536) {
    bytes.extend_from_slice(&0u32.to_le_bytes());
  }
  bytes.extend_from_slice(&Sha256::digest(&bytes));
  ram::wire::decode(&mut &bytes[..], (guest, size), || Ok(())).unwrap()
}

fn command(size: usize) -> flash::snapshot::State {
  flash::snapshot::capture(&flash::create(vec![0; size], 65536).unwrap())
}

#[test]
fn fresh_store_reopens_with_captured_bytes_and_never_replaces_existing_state() {
  let root = tempfile::tempdir().unwrap();
  let path = root.path().join("restored");
  let bank = memory(0x4000000, SIZE);
  let state = command(SIZE);
  let store = variables::from_snapshot(&path, &bank, &state, || Ok(())).unwrap();
  assert!(variables::bytes(&store).iter().all(|byte| *byte == 0));
  assert!(variables::open(&path).is_err());
  assert!(variables::from_snapshot(&path, &bank, &state, || Ok(())).is_err());
  drop(store);
  let mut store = variables::open(&path).unwrap();
  variables::commit(&mut store, 0, &[1, 2, 3, 4]).unwrap();
  drop(store);
  assert!(variables::from_snapshot(&path, &bank, &state, || Ok(())).is_err());
  let store = variables::open(&path).unwrap();
  assert_eq!(&variables::bytes(&store)[..4], &[1, 2, 3, 4]);
}

#[test]
fn authorization_loss_before_creation_leaves_no_store_but_after_creation_preserves_it() {
  let root = tempfile::tempdir().unwrap();
  let bank = memory(0x4000000, SIZE);
  let state = command(SIZE);
  let early = root.path().join("early");
  let mut checks = 0;
  assert!(variables::from_snapshot(&early, &bank, &state, || {
    checks += 1;
    anyhow::ensure!(checks < 100, "revoked");
    Ok(())
  })
  .is_err());
  assert_eq!(checks, 100);
  assert!(!early.exists());
  let late = root.path().join("late");
  assert!(variables::from_snapshot(&late, &bank, &state, || {
    anyhow::ensure!(!late.exists(), "revoked after creation");
    Ok(())
  })
  .is_err());
  let preserved = variables::open(&late).unwrap();
  assert!(variables::bytes(&preserved).iter().all(|byte| *byte == 0));
}

#[test]
fn wrong_geometry_or_flash_bank_rejects_without_allocating_a_store() {
  let root = tempfile::tempdir().unwrap();
  let path = root.path().join("invalid");
  let state = command(SIZE);
  for (guest, size) in [(0, SIZE), (0x4000000, 65536)] {
    assert!(variables::from_snapshot(&path, &memory(guest, size), &state, || Ok(())).is_err());
    assert!(!path.exists());
  }
  assert!(
    variables::from_snapshot(&path, &memory(0x4000000, SIZE), &command(65536), || Ok(())).is_err()
  );
  assert!(!path.exists());
}

#[test]
fn relative_snapshot_restore_survives_stage_publication() {
  use std::{
    fs::{self, File, Permissions},
    os::unix::fs::PermissionsExt,
  };
  let root = tempfile::tempdir().unwrap();
  let stage = root.path().join("stage");
  fs::create_dir(&stage).unwrap();
  fs::set_permissions(&stage, Permissions::from_mode(0o700)).unwrap();
  let parent = File::open(&stage).unwrap();
  let bank = memory(0x4000000, SIZE);
  let state = command(SIZE);
  let mut store = variables::from_snapshot_at(&parent, &bank, &state, || Ok(())).unwrap();
  let published = root.path().join("published");
  fs::rename(&stage, &published).unwrap();
  variables::commit(&mut store, 0, &[9; 4]).unwrap();
  assert!(variables::from_snapshot_at(&parent, &bank, &state, || Ok(())).is_err());
  drop(store);
  let reopened = variables::open_at(&parent).unwrap();
  assert_eq!(&variables::bytes(&reopened)[..4], &[9; 4]);
  assert!(variables::bytes(&reopened)[4..]
    .iter()
    .all(|byte| *byte == 0));
}
