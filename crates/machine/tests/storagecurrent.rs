#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
use fs2::FileExt;
use machine::{devices::virtio::block, snapshot::storage::Current};
use std::{
  fs::{self, File, OpenOptions, Permissions},
  os::unix::fs::{FileExt as _, OpenOptionsExt, PermissionsExt},
};

fn file(root: &std::path::Path) -> File {
  fs::set_permissions(root, Permissions::from_mode(0o700)).unwrap();
  let file = OpenOptions::new()
    .read(true)
    .write(true)
    .create_new(true)
    .mode(0o600)
    .open(root.join("disk"))
    .unwrap();
  file.set_len(4096).unwrap();
  file
}

#[test]
fn current_disk_preserves_modified_bytes_and_transfers_exclusive_ownership() {
  let root = tempfile::tempdir().unwrap();
  let file = file(root.path());
  file.write_all_at(b"modified", 0).unwrap();
  let directory = File::open(root.path()).unwrap();
  let current = Current::open(&directory, || Ok(())).unwrap();
  let duplicate = current.file().try_clone().unwrap();
  assert!(file.try_lock_exclusive().is_err());
  let disk = current.disk([b'd'; 20], || Ok(())).unwrap();
  assert_eq!(block::capacity(&disk), 8);
  assert!(!block::readonly(&disk));
  assert!(file.try_lock_exclusive().is_err());
  drop(disk);
  file.try_lock_exclusive().unwrap();
  FileExt::unlock(&file).unwrap();
  let mut bytes = [0; 8];
  duplicate.read_exact_at(&mut bytes, 0).unwrap();
  assert_eq!(&bytes, b"modified");
}

#[test]
fn failed_adoption_unlocks_retained_duplicates_and_never_changes_contents() {
  let root = tempfile::tempdir().unwrap();
  let file = file(root.path());
  file.write_all_at(b"retained", 0).unwrap();
  let directory = File::open(root.path()).unwrap();
  let current = Current::open(&directory, || Ok(())).unwrap();
  let _duplicate = current.file().try_clone().unwrap();
  assert!(current
    .disk([b'd'; 20], || anyhow::bail!("revoked"))
    .is_err());
  file.try_lock_exclusive().unwrap();
  FileExt::unlock(&file).unwrap();
  assert_eq!(
    fs::read(root.path().join("disk")).unwrap()[..8],
    *b"retained"
  );
}

#[test]
fn replaced_leaf_and_changed_capacity_reject_before_adoption() {
  let root = tempfile::tempdir().unwrap();
  let file = file(root.path());
  let directory = File::open(root.path()).unwrap();
  let current = Current::open(&directory, || Ok(())).unwrap();
  file.set_len(8192).unwrap();
  assert!(current.check(|| Ok(())).is_err());
  file.set_len(4096).unwrap();
  fs::rename(root.path().join("disk"), root.path().join("saved")).unwrap();
  let replacement = OpenOptions::new()
    .read(true)
    .write(true)
    .create_new(true)
    .mode(0o600)
    .open(root.path().join("disk"))
    .unwrap();
  replacement.set_len(4096).unwrap();
  assert!(current.disk([b'd'; 20], || Ok(())).is_err());
  file.try_lock_exclusive().unwrap();
  FileExt::unlock(&file).unwrap();
}

#[test]
fn unsafe_current_disk_types_modes_links_and_sizes_reject() {
  use std::os::unix::fs::symlink;
  let root = tempfile::tempdir().unwrap();
  let file = file(root.path());
  let directory = File::open(root.path()).unwrap();
  file.set_len(513).unwrap();
  assert!(Current::open(&directory, || Ok(())).is_err());
  file.set_len(4096).unwrap();
  file.set_permissions(Permissions::from_mode(0o644)).unwrap();
  assert!(Current::open(&directory, || Ok(())).is_err());
  file.set_permissions(Permissions::from_mode(0o600)).unwrap();
  fs::hard_link(root.path().join("disk"), root.path().join("alias")).unwrap();
  assert!(Current::open(&directory, || Ok(())).is_err());
  fs::remove_file(root.path().join("alias")).unwrap();
  fs::rename(root.path().join("disk"), root.path().join("saved")).unwrap();
  symlink("saved", root.path().join("disk")).unwrap();
  assert!(Current::open(&directory, || Ok(())).is_err());
  fs::remove_file(root.path().join("disk")).unwrap();
  fs::create_dir(root.path().join("disk")).unwrap();
  assert!(Current::open(&directory, || Ok(())).is_err());
}
