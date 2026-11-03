#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
use fs2::FileExt;
use machine::snapshot::storage;
use std::{
  fs::{File, OpenOptions, Permissions},
  os::unix::fs::{FileExt as _, MetadataExt, PermissionsExt},
};

fn source(root: &std::path::Path) -> File {
  let file = OpenOptions::new()
    .read(true)
    .write(true)
    .create_new(true)
    .open(root.join("source"))
    .unwrap();
  file.set_len(64 << 20).unwrap();
  file.write_all_at(b"original", 0).unwrap();
  file.sync_all().unwrap();
  file
}

fn stage(root: &std::path::Path) -> (std::path::PathBuf, File) {
  let path = root.join("stage");
  std::fs::create_dir(&path).unwrap();
  std::fs::set_permissions(&path, Permissions::from_mode(0o700)).unwrap();
  let file = File::open(&path).unwrap();
  (path, file)
}

#[test]
fn writable_restore_clone_persists_without_mutating_capture_or_source() {
  let root = tempfile::tempdir().unwrap();
  let source = source(root.path());
  let saved = storage::capture(&source).unwrap();
  source.write_all_at(b"advanced", 0).unwrap();
  let (path, parent) = stage(root.path());
  let prepared = saved.prepare(&parent, || Ok(())).unwrap();
  let metadata = prepared.file().metadata().unwrap();
  assert_ne!(metadata.ino(), source.metadata().unwrap().ino());
  assert_eq!(metadata.mode() & 0o7777, 0o600);
  assert!(metadata.blocks() * 512 < metadata.len() / 2);
  let mut bytes = [0; 8];
  prepared.file().read_exact_at(&mut bytes, 0).unwrap();
  assert_eq!(&bytes, b"original");
  prepared.file().write_all_at(b"restored", 0).unwrap();
  prepared.file().sync_all().unwrap();
  saved.read(0, &mut bytes).unwrap();
  assert_eq!(&bytes, b"original");
  source.read_exact_at(&mut bytes, 0).unwrap();
  assert_eq!(&bytes, b"advanced");
  drop(prepared);
  drop(saved);
  File::open(path.join("disk"))
    .unwrap()
    .read_exact_at(&mut bytes, 0)
    .unwrap();
  assert_eq!(&bytes, b"restored");
}

#[test]
fn exclusive_owner_unlocks_on_drop_even_with_a_retained_duplicate() {
  let root = tempfile::tempdir().unwrap();
  let saved = storage::capture(&source(root.path())).unwrap();
  let (path, parent) = stage(root.path());
  let prepared = saved.prepare(&parent, || Ok(())).unwrap();
  let duplicate = prepared.file().try_clone().unwrap();
  let contender = OpenOptions::new()
    .read(true)
    .write(true)
    .open(path.join("disk"))
    .unwrap();
  assert!(contender.try_lock_exclusive().is_err());
  drop(prepared);
  contender.try_lock_exclusive().unwrap();
  FileExt::unlock(&contender).unwrap();
  assert!(duplicate.metadata().unwrap().is_file());
}

#[test]
fn existing_destinations_and_nonprivate_stages_are_preserved() {
  for fault in 0..3 {
    let root = tempfile::tempdir().unwrap();
    let saved = storage::capture(&source(root.path())).unwrap();
    let (path, parent) = stage(root.path());
    if fault == 0 {
      std::fs::write(path.join("disk"), b"retained").unwrap();
    }
    if fault == 1 {
      std::os::unix::fs::symlink(root.path().join("source"), path.join("disk")).unwrap();
    }
    if fault == 2 {
      std::fs::set_permissions(&path, Permissions::from_mode(0o755)).unwrap();
    }
    assert!(saved.prepare(&parent, || Ok(())).is_err());
    if fault == 0 {
      assert_eq!(std::fs::read(path.join("disk")).unwrap(), b"retained");
    }
    if fault == 1 {
      assert!(path.join("disk").is_symlink());
    }
    if fault == 2 {
      assert!(!path.join("disk").exists());
    }
  }
}

#[test]
fn cancellation_preserves_written_stage_and_releases_any_acquired_lock() {
  for deny in 0..3 {
    let root = tempfile::tempdir().unwrap();
    let saved = storage::capture(&source(root.path())).unwrap();
    let (path, parent) = stage(root.path());
    let mut checks = 0;
    assert!(saved
      .prepare(&parent, || {
        let current = checks;
        checks += 1;
        anyhow::ensure!(current != deny, "cancelled");
        Ok(())
      })
      .is_err());
    assert_eq!(path.join("disk").exists(), deny > 0);
    if deny == 2 {
      let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path.join("disk"))
        .unwrap();
      file.try_lock_exclusive().unwrap();
      FileExt::unlock(&file).unwrap();
    }
  }
}

#[test]
fn replacement_during_preparation_rejects_without_redirecting_the_owned_inode() {
  let root = tempfile::tempdir().unwrap();
  let saved = storage::capture(&source(root.path())).unwrap();
  let (path, parent) = stage(root.path());
  let mut checks = 0;
  assert!(saved
    .prepare(&parent, || {
      checks += 1;
      if checks == 2 {
        std::fs::rename(path.join("disk"), path.join("preserved"))?;
        let foreign = OpenOptions::new()
          .write(true)
          .create_new(true)
          .open(path.join("disk"))?;
        foreign.set_len(64 << 20)?;
        foreign.set_permissions(Permissions::from_mode(0o600))?;
      }
      Ok(())
    })
    .is_err());
  let mut bytes = [0; 8];
  File::open(path.join("preserved"))
    .unwrap()
    .read_exact_at(&mut bytes, 0)
    .unwrap();
  assert_eq!(&bytes, b"original");
  File::open(path.join("disk"))
    .unwrap()
    .read_exact_at(&mut bytes, 0)
    .unwrap();
  assert_eq!(bytes, [0; 8]);
}

#[test]
fn reopened_prepared_disk_requires_unchanged_bytes_and_releases_failed_ownership() {
  let root = tempfile::tempdir().unwrap();
  let saved = storage::capture(&source(root.path())).unwrap();
  let (path, parent) = stage(root.path());
  let prepared = saved.prepare(&parent, || Ok(())).unwrap();
  assert!(saved.reopen_prepared(&parent, || Ok(())).is_err());
  drop(prepared);
  let reopened = saved.reopen_prepared(&parent, || Ok(())).unwrap();
  reopened.file().write_all_at(b"modified", 0).unwrap();
  reopened.file().sync_all().unwrap();
  drop(reopened);
  assert!(saved.reopen_prepared(&parent, || Ok(())).is_err());
  let file = OpenOptions::new()
    .read(true)
    .write(true)
    .open(path.join("disk"))
    .unwrap();
  file.try_lock_exclusive().unwrap();
  FileExt::unlock(&file).unwrap();
  let mut bytes = [0; 8];
  saved.read(0, &mut bytes).unwrap();
  assert_eq!(&bytes, b"original");
  file.read_exact_at(&mut bytes, 0).unwrap();
  assert_eq!(&bytes, b"modified");
}

#[test]
fn reopened_prepared_disk_checks_authorization_during_content_validation() {
  let root = tempfile::tempdir().unwrap();
  let saved = storage::capture(&source(root.path())).unwrap();
  let (_, parent) = stage(root.path());
  drop(saved.prepare(&parent, || Ok(())).unwrap());
  let mut total = 0;
  drop(
    saved
      .reopen_prepared(&parent, || {
        total += 1;
        Ok(())
      })
      .unwrap(),
  );
  for revoked in 1..=total {
    let mut checks = 0;
    assert!(saved
      .reopen_prepared(&parent, || {
        checks += 1;
        anyhow::ensure!(checks < revoked, "revoked");
        Ok(())
      })
      .is_err());
    assert_eq!(checks, revoked);
  }
  drop(saved.reopen_prepared(&parent, || Ok(())).unwrap());
}
