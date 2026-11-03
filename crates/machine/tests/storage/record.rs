use super::*;
use std::os::unix::fs::PermissionsExt;

fn private() -> tempfile::TempDir {
  tempfile::Builder::new()
    .permissions(fs::Permissions::from_mode(0o700))
    .tempdir()
    .unwrap()
}

fn image() -> storage::Image {
  sized_image(1024 * 1024)
}

fn sized_image(size: u64) -> storage::Image {
  let source = tempfile::tempfile().unwrap();
  source.set_len(size).unwrap();
  source.write_all_at(b"first", 0).unwrap();
  source.write_all_at(b"last", size - 4).unwrap();
  storage::capture(&source).unwrap()
}

#[test]
fn saved_images_outlive_original_capture_and_reopen_into_independent_sparse_forks() {
  let root = private();
  let directory = File::open(root.path()).unwrap();
  let saved = sized_image(64 * 1024 * 1024);
  let size = saved.size();
  let bytes = storage::record::save(&saved, &directory, || Ok(())).unwrap();
  assert!(storage::record::save(&saved, &directory, || Ok(())).is_err());
  drop(saved);
  for _ in 0..2 {
    let saved = storage::record::load(&directory, &bytes, size, || Ok(())).unwrap();
    let fork = saved.fork().unwrap();
    assert!(fork.file().metadata().unwrap().blocks() * 512 < size / 2);
    let mut value = [0; 5];
    fork.file().read_exact_at(&mut value, 0).unwrap();
    assert_eq!(&value, b"first");
    fork.file().write_all_at(b"other", 0).unwrap();
    saved.read(0, &mut value).unwrap();
    assert_eq!(&value, b"first");
    saved.read(size - 4, &mut value[..4]).unwrap();
    assert_eq!(&value[..4], b"last");
  }
  assert_eq!(
    fs::metadata(root.path().join("image"))
      .unwrap()
      .permissions()
      .mode()
      & 0o7777,
    0o400
  );
}

#[test]
fn unsafe_files_and_damaged_images_or_receipts_reject() {
  let root = private();
  let directory = File::open(root.path()).unwrap();
  let saved = image();
  let size = saved.size();
  let bytes = storage::record::save(&saved, &directory, || Ok(())).unwrap();
  assert!(storage::record::load(&directory, &bytes, size + 1, || Ok(())).is_err());
  for end in [0, 4, bytes.len() - 1] {
    assert!(storage::record::load(&directory, &bytes[..end], size, || Ok(())).is_err());
  }
  let mut changed = bytes;
  changed[20] ^= 1;
  assert!(storage::record::load(&directory, &changed, size, || Ok(())).is_err());
  let path = root.path().join("image");
  fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
  assert!(storage::record::load(&directory, &bytes, size, || Ok(())).is_err());
  let writer = OpenOptions::new().write(true).open(&path).unwrap();
  writer.write_all_at(b"wrong", 0).unwrap();
  fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
  assert!(storage::record::load(&directory, &bytes, size, || Ok(())).is_err());
  writer.write_all_at(b"first", 0).unwrap();
  assert!(storage::record::load(&directory, &bytes, size, || Ok(())).is_ok());
  let extra = root.path().join("extra");
  fs::hard_link(&path, &extra).unwrap();
  assert!(storage::record::load(&directory, &bytes, size, || Ok(())).is_err());
  fs::remove_file(extra).unwrap();
  fs::rename(&path, root.path().join("real")).unwrap();
  std::os::unix::fs::symlink("real", &path).unwrap();
  assert!(storage::record::load(&directory, &bytes, size, || Ok(())).is_err());
  fs::remove_file(&path).unwrap();
  fs::create_dir(&path).unwrap();
  assert!(storage::record::load(&directory, &bytes, size, || Ok(())).is_err());
}

#[test]
fn revocation_rejects_at_each_save_and_load_checkpoint() {
  let saved = image();
  let baseline = private();
  let directory = File::open(baseline.path()).unwrap();
  let mut savechecks = 0;
  let bytes = storage::record::save(&saved, &directory, || {
    savechecks += 1;
    Ok(())
  })
  .unwrap();
  let mut loadchecks = 0;
  storage::record::load(&directory, &bytes, saved.size(), || {
    loadchecks += 1;
    Ok(())
  })
  .unwrap();
  for stop in 1..=savechecks {
    let stage = private();
    let directory = File::open(stage.path()).unwrap();
    let mut checks = 0;
    assert!(storage::record::save(&saved, &directory, || {
      checks += 1;
      anyhow::ensure!(checks < stop, "revoked");
      Ok(())
    })
    .is_err());
  }
  for stop in 1..=loadchecks {
    let mut checks = 0;
    assert!(storage::record::load(&directory, &bytes, saved.size(), || {
      checks += 1;
      anyhow::ensure!(checks < stop, "revoked");
      Ok(())
    })
    .is_err());
  }
}

#[test]
fn pinned_directory_survives_path_replacement_and_public_directories_reject() {
  let root = private();
  let stage = root.path().join("stage");
  fs::create_dir(&stage).unwrap();
  fs::set_permissions(&stage, fs::Permissions::from_mode(0o700)).unwrap();
  let directory = File::open(&stage).unwrap();
  fs::rename(&stage, root.path().join("retired")).unwrap();
  fs::create_dir(&stage).unwrap();
  let saved = image();
  let bytes = storage::record::save(&saved, &directory, || Ok(())).unwrap();
  assert!(!stage.join("image").exists());
  storage::record::load(&directory, &bytes, saved.size(), || Ok(())).unwrap();
  let foreign = File::open(&stage).unwrap();
  assert!(storage::record::save(&saved, &foreign, || Ok(())).is_err());
}
