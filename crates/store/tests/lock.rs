use fs2::FileExt;
use std::fs::OpenOptions;

#[test]
fn ownership_ends_even_when_a_duplicate_descriptor_remains_open() {
  let root = tempfile::tempdir().unwrap();
  let path = root.path().join("lease");
  let open = || {
    OpenOptions::new()
      .create(true)
      .truncate(false)
      .read(true)
      .write(true)
      .open(&path)
      .unwrap()
  };
  let lease = store::lock::exclusive(open()).unwrap();
  let inherited = lease.try_clone().unwrap();
  assert!(store::lock::exclusive(open()).is_err());
  drop(lease);
  let next = store::lock::exclusive(open()).unwrap();
  // closing the old descriptor must not release a new owner's independent lock.
  drop(inherited);
  assert!(store::lock::exclusive(open()).is_err());
  drop(next);
  assert!(store::lock::exclusive(open()).is_ok());
}

#[test]
#[cfg(unix)]
fn file_close_without_unlock_retains_a_duplicated_flock() {
  let root = tempfile::tempdir().unwrap();
  let path = root.path().join("raw");
  let file = OpenOptions::new()
    .create_new(true)
    .read(true)
    .write(true)
    .open(&path)
    .unwrap();
  file.try_lock_exclusive().unwrap();
  let inherited = file.try_clone().unwrap();
  drop(file);
  let probe = OpenOptions::new()
    .read(true)
    .write(true)
    .open(&path)
    .unwrap();
  assert!(probe.try_lock_exclusive().is_err());
  drop(inherited);
  probe.try_lock_exclusive().unwrap();
}
