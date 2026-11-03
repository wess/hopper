#![cfg(unix)]

use guest::files;
use std::{fs, os::unix::fs::symlink};

#[test]
fn ranges_are_bounded_and_recover_binary_bytes_without_text_conversion() {
  let root = tempfile::tempdir().unwrap();
  let path = root.path().join("source");
  let data = (0..files::CHUNK + 17)
    .map(|index| index as u8)
    .collect::<Vec<_>>();
  fs::write(&path, &data).unwrap();
  let reader = files::open(&path).unwrap();
  assert_eq!(reader.stamp().size, data.len() as u64);
  let first = reader.read(0, files::CHUNK).unwrap();
  assert_eq!(first.offset, 0);
  assert_eq!(first.bytes, data[..files::CHUNK]);
  assert!(!first.eof);
  let last = reader.read(files::CHUNK as u64, files::CHUNK).unwrap();
  assert_eq!(last.bytes, data[files::CHUNK..]);
  assert!(last.eof);
  let empty = reader.read(data.len() as u64, 1).unwrap();
  assert!(empty.bytes.is_empty() && empty.eof);
  for (offset, length) in [(0, 0), (0, files::CHUNK + 1), (u64::MAX, 1)] {
    assert!(reader.read(offset, length).is_err());
  }
}

#[test]
fn retained_handle_never_redirects_to_a_replacement_path() {
  let root = tempfile::tempdir().unwrap();
  let folder = root.path().join("folder");
  fs::create_dir(&folder).unwrap();
  let path = folder.join("source");
  fs::write(&path, b"original").unwrap();
  let reader = files::open(&path).unwrap();
  fs::rename(&folder, root.path().join("retained")).unwrap();
  fs::create_dir(&folder).unwrap();
  fs::write(&path, b"replacement").unwrap();
  assert_eq!(reader.read(0, 64).unwrap().bytes, b"original");
  assert_eq!(fs::read(path).unwrap(), b"replacement");
}

#[test]
fn changed_sources_and_nonregular_or_symlink_targets_reject() {
  let root = tempfile::tempdir().unwrap();
  let path = root.path().join("source");
  fs::write(&path, b"original").unwrap();
  let reader = files::open(&path).unwrap();
  fs::write(&path, b"changed content").unwrap();
  assert!(reader.read(0, 64).is_err());
  let link = root.path().join("link");
  symlink(&path, &link).unwrap();
  for path in [
    root.path(),
    &link,
    std::path::Path::new("relative"),
    std::path::Path::new("/dev/null"),
  ] {
    assert!(files::open(path).is_err());
  }
  #[cfg(target_os = "linux")]
  {
    assert!(files::open(std::path::Path::new("/proc/self/environ")).is_err());
    assert!(files::open(std::path::Path::new("/sys/kernel/uevent_seqnum")).is_err());
  }
}
