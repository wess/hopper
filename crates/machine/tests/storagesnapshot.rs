#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use machine::snapshot::storage;
use std::{
  fs::{self, File, OpenOptions},
  os::unix::fs::{FileExt, MetadataExt},
  time::Instant,
};

#[path = "storage/record.rs"]
mod record;

#[test]
fn sparse_clones_pin_source_inode_and_remain_independent_in_both_directions() {
  let root = tempfile::tempdir().unwrap();
  let path = root.path().join("source");
  let source = OpenOptions::new()
    .read(true)
    .write(true)
    .create_new(true)
    .open(&path)
    .unwrap();
  let size = 64 * 1024 * 1024;
  source.set_len(size).unwrap();
  source.write_all_at(b"first", 0).unwrap();
  source.write_all_at(b"last", size - 4).unwrap();
  source.sync_all().unwrap();
  let started = Instant::now();
  let saved = storage::capture(&source).unwrap();
  eprintln!("64 MiB sparse APFS capture: {:?}", started.elapsed());
  assert_eq!(saved.size(), size);
  let mut bytes = [0; 5];
  source.write_all_at(b"newer", 0).unwrap();
  let retired = root.path().join("retired");
  fs::rename(&path, &retired).unwrap();
  fs::write(&path, b"foreign").unwrap();
  let current = storage::capture(&source).unwrap();
  current.read(0, &mut bytes).unwrap();
  assert_eq!(&bytes, b"newer");
  saved.read(0, &mut bytes).unwrap();
  assert_eq!(&bytes, b"first");
  for _ in 0..2 {
    let restored = saved.fork().unwrap();
    let metadata = restored.file().metadata().unwrap();
    assert_eq!(metadata.len(), size);
    assert_eq!(metadata.mode() & 0o777, 0o600);
    assert_ne!(metadata.ino(), source.metadata().unwrap().ino());
    assert!(
      metadata.blocks() * 512 < size / 2,
      "Sparse clone allocated its holes"
    );
    restored.file().read_exact_at(&mut bytes, 0).unwrap();
    assert_eq!(&bytes, b"first");
    restored.file().write_all_at(b"fork!", 0).unwrap();
    saved.read(0, &mut bytes).unwrap();
    assert_eq!(&bytes, b"first");
    let retained = restored.file().try_clone().unwrap();
    drop(restored);
    assert_eq!(retained.metadata().unwrap().nlink(), 0);
  }
  drop(source);
  fs::remove_file(retired).unwrap();
  saved.read(size - 4, &mut bytes[..4]).unwrap();
  assert_eq!(&bytes[..4], b"last");
  assert_eq!(fs::read(path).unwrap(), b"foreign");
  assert!(saved.read(u64::MAX, &mut bytes).is_err());
  assert!(saved.read(size, &mut bytes).is_err());
  assert!(saved.read(0, &mut vec![0; 65537]).is_err());
  saved.read(size, &mut []).unwrap();
}

#[test]
fn invalid_sources_reject_without_changing_them() {
  let root = tempfile::tempdir().unwrap();
  assert!(storage::capture(&File::open(root.path()).unwrap()).is_err());
  let empty = tempfile::tempfile().unwrap();
  assert!(storage::capture(&empty).is_err());
  assert_eq!(empty.metadata().unwrap().len(), 0);
}

#[test]
fn recovered_readonly_images_use_readonly_descriptors_and_independent_positions() {
  use std::io::Read;
  let source = tempfile::tempfile().unwrap();
  source.set_len(4096).unwrap();
  source.write_all_at(b"abcdef", 0).unwrap();
  let saved = storage::capture(&source).unwrap();
  let first = saved.fork_readonly().unwrap();
  let second = saved.fork_readonly().unwrap();
  assert_eq!(first.file().metadata().unwrap().mode() & 0o777, 0o400);
  assert!(first.file().write_all_at(b"x", 0).is_err());
  let mut a = [0; 3];
  let mut b = [0; 3];
  first.file().read_exact(&mut a).unwrap();
  second.file().read_exact(&mut b).unwrap();
  assert_eq!(a, *b"abc");
  assert_eq!(b, *b"abc");
  first.file().read_exact(&mut a).unwrap();
  assert_eq!(a, *b"def");
}
