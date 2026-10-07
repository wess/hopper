#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use machine::vz::sharing::Directory;
use std::os::unix::fs::symlink;

#[test]
fn share_requires_a_named_absolute_directory_without_a_final_symlink() {
  let root = tempfile::tempdir().unwrap();
  let directory = root.path().join("folder");
  std::fs::create_dir(&directory).unwrap();
  for name in ["", ".", "..", "a/b", "a\\b", "a\0b", "a:b", "a b"] {
    assert!(Directory::open(name, &directory, true).is_err());
  }
  assert!(Directory::open(&"x".repeat(65), &directory, true).is_err());
  assert!(Directory::open("folder", std::path::Path::new("relative"), true).is_err());
  let regular = root.path().join("file");
  std::fs::write(&regular, b"owned synthetic file").unwrap();
  assert!(Directory::open("folder", &regular, true).is_err());
  let linked = root.path().join("linked");
  symlink(&directory, &linked).unwrap();
  assert!(Directory::open("folder", &linked, true).is_err());
  assert!(Directory::open("folder", &root.path().join("missing"), true).is_err());
  assert!(Directory::open("work.1", &directory, true).is_ok());
  assert!(Directory::open("work_2", &directory, false).is_ok());
}
