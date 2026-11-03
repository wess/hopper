#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
use machine::snapshot::storage;
use std::{
  fs::{File, OpenOptions, Permissions},
  os::unix::fs::{FileExt, PermissionsExt},
};

fn fixture() -> (tempfile::TempDir, storage::Image, storage::Prepared) {
  let root = tempfile::tempdir().unwrap();
  std::fs::set_permissions(root.path(), Permissions::from_mode(0o700)).unwrap();
  let source = tempfile::tempfile().unwrap();
  source.set_len(65536 * 3).unwrap();
  source.write_all_at(b"saved", 0).unwrap();
  let image = storage::capture(&source).unwrap();
  let prepared = image
    .prepare(&File::open(root.path()).unwrap(), || Ok(()))
    .unwrap();
  (root, image, prepared)
}

#[test]
fn original_capture_and_all_bytes_are_required_for_adoption() {
  let (_root, image, prepared) = fixture();
  prepared.validate(&image, || Ok(())).unwrap();
  let other = tempfile::tempfile().unwrap();
  other.set_len(65536 * 3).unwrap();
  other.write_all_at(b"saved", 0).unwrap();
  let foreign = storage::capture(&other).unwrap();
  assert!(prepared.validate(&foreign, || Ok(())).is_err());
  prepared.file().write_all_at(b"changed", 65536).unwrap();
  assert!(prepared.validate(&image, || Ok(())).is_err());
}

#[test]
fn namespace_replacement_and_unsafe_metadata_reject_without_changing_captured_data() {
  for fault in 0..4 {
    let (root, image, prepared) = fixture();
    let path = root.path().join("disk");
    match fault {
      0 => {
        std::fs::rename(&path, root.path().join("preserved")).unwrap();
        let file = OpenOptions::new()
          .write(true)
          .create_new(true)
          .open(&path)
          .unwrap();
        file.set_len(65536 * 3).unwrap();
        file.set_permissions(Permissions::from_mode(0o600)).unwrap();
      }
      1 => {
        prepared.file().set_len(65536).unwrap();
      }
      2 => {
        prepared
          .file()
          .set_permissions(Permissions::from_mode(0o644))
          .unwrap();
      }
      _ => {
        std::fs::hard_link(&path, root.path().join("linked")).unwrap();
      }
    }
    assert!(prepared.validate(&image, || Ok(())).is_err());
    let mut bytes = [0; 5];
    image.read(0, &mut bytes).unwrap();
    assert_eq!(&bytes, b"saved");
  }
}

#[test]
fn cancellation_brackets_every_chunk_and_preserves_owned_stage() {
  for deny in 0..5 {
    let (root, image, prepared) = fixture();
    let mut checks = 0;
    assert!(prepared
      .validate(&image, || {
        let current = checks;
        checks += 1;
        anyhow::ensure!(current != deny, "revoked");
        Ok(())
      })
      .is_err());
    assert!(root.path().join("disk").exists());
    prepared.validate(&image, || Ok(())).unwrap();
  }
}
