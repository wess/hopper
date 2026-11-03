#![cfg(unix)]
use guest::files::browse;
use model::guest::files::Kind;
use std::{
  fs,
  os::unix::{ffi::OsStringExt, fs::symlink},
};

#[test]
fn pages_preserve_binary_names_and_never_follow_entry_symlinks() {
  let root = tempfile::tempdir().unwrap();
  fs::write(root.path().join("a"), b"data").unwrap();
  fs::create_dir(root.path().join("b")).unwrap();
  symlink("/does/not/exist", root.path().join("c")).unwrap();
  let raw_bytes = if cfg!(target_os = "linux") {
    vec![255, 254]
  } else {
    "é".as_bytes().to_vec()
  };
  let raw = std::ffi::OsString::from_vec(raw_bytes.clone());
  fs::write(root.path().join(raw), b"binary").unwrap();
  let directory = browse::open(root.path()).unwrap();
  let first = directory.read(2, None, None).unwrap();
  assert_eq!(first.entries[0].name, b"a");
  assert_eq!(first.entries[0].kind, Kind::File);
  assert_eq!(first.entries[1].kind, Kind::Directory);
  assert_eq!(first.next.as_deref(), Some(b"b".as_slice()));
  let second = directory
    .read(2, first.next.as_deref(), Some(first.stamp))
    .unwrap();
  assert_eq!(second.entries[0].name, b"c");
  assert_eq!(second.entries[0].kind, Kind::Symlink);
  assert_eq!(second.entries[1].name, raw_bytes);
  assert!(second.next.is_none());
}

#[test]
fn retained_directory_rejects_mutation_and_never_redirects_to_replacement() {
  let root = tempfile::tempdir().unwrap();
  let path = root.path().join("original");
  fs::create_dir(&path).unwrap();
  fs::write(path.join("old"), b"old").unwrap();
  let directory = browse::open(&path).unwrap();
  let first = directory.read(1, None, None).unwrap();
  fs::write(path.join("added"), b"added").unwrap();
  assert!(directory.read(1, None, Some(first.stamp)).is_err());
  let directory = browse::open(&path).unwrap();
  fs::rename(&path, root.path().join("retained")).unwrap();
  fs::create_dir(&path).unwrap();
  fs::write(path.join("foreign"), b"foreign").unwrap();
  if let Ok(listing) = directory.read(32, None, None) {
    assert!(listing.entries.iter().all(|entry| entry.name != b"foreign"));
  }
  symlink(&path, root.path().join("link")).unwrap();
  assert!(browse::open(&root.path().join("link")).is_err());
  assert!(browse::open(&path.join("foreign")).is_err());
}

#[test]
fn maximum_page_fits_job_output_and_invalid_cursors_reject() {
  let root = tempfile::tempdir().unwrap();
  for index in 0..40 {
    fs::write(
      root.path().join(format!("{index:03}{}", "x".repeat(252))),
      b"",
    )
    .unwrap();
  }
  let directory = browse::open(root.path()).unwrap();
  let page = directory.read(32, None, None).unwrap();
  assert_eq!(page.entries.len(), 32);
  assert!(page.next.is_some());
  assert!(serde_json::to_vec(&page).unwrap().len() < 64 << 10);
  for limit in [0, 33] {
    assert!(directory.read(limit, None, None).is_err());
  }
  for after in [b"".as_slice(), b"..", b"a/b", b"a\0b"] {
    assert!(directory.read(1, Some(after), None).is_err());
  }
  let mut foreign = page.stamp;
  foreign.inode += 1;
  assert!(directory.read(1, None, Some(foreign)).is_err());
}
