#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
use machine::snapshot::storage;
use sha2::{Digest, Sha256};
use std::{
  fs::{self, File},
  io::{Seek, SeekFrom},
  os::unix::fs::{FileExt, PermissionsExt},
};

fn private() -> tempfile::TempDir {
  tempfile::Builder::new()
    .permissions(fs::Permissions::from_mode(0o700))
    .tempdir()
    .unwrap()
}

fn save(file: &File) -> ([u8; storage::record::SIZE], tempfile::TempDir) {
  let root = private();
  let image = storage::capture(file).unwrap();
  let record = storage::record::save(&image, &File::open(root.path()).unwrap(), || Ok(())).unwrap();
  (record, root)
}

#[test]
fn canonical_checksum_matches_independent_cross_boundary_and_partial_chunk_vector() {
  let mut data = vec![0; 131079];
  data[65535..65540].copy_from_slice(b"alpha");
  *data.last_mut().unwrap() = 127;
  let file = tempfile::tempfile().unwrap();
  file.write_all_at(&data, 0).unwrap();
  let (record, root) = save(&file);
  assert_eq!(&record[..4], b"HDS2");
  let digest: String = record[12..44]
    .iter()
    .map(|byte| format!("{byte:02x}"))
    .collect();
  assert_eq!(
    digest,
    "e2640c0d055b15f0a0d641a308ac7dca961524d3aefee54550e9e6999d8b623a"
  );
  storage::record::load(
    &File::open(root.path()).unwrap(),
    &record,
    data.len() as u64,
    || Ok(()),
  )
  .unwrap();
}

#[test]
fn allocated_zeros_and_filesystem_holes_have_identical_content_digests() {
  let sparse = tempfile::tempfile().unwrap();
  let dense = tempfile::tempfile().unwrap();
  let size = 2 << 20;
  sparse.set_len(size).unwrap();
  dense.write_all_at(&vec![0; size as usize], 0).unwrap();
  for file in [&sparse, &dense] {
    file.write_all_at(b"first", 0).unwrap();
    file.write_all_at(b"last", size - 4).unwrap();
    file.sync_all().unwrap();
  }
  let (left, _left) = save(&sparse);
  let (right, _right) = save(&dense);
  assert_eq!(left, right);
}

#[test]
fn large_sparse_images_skip_holes_and_detect_new_data_in_previously_empty_regions() {
  let file = tempfile::tempfile().unwrap();
  let size = 64 << 30;
  file.set_len(size).unwrap();
  file.write_all_at(b"first", 0).unwrap();
  file.write_all_at(b"last", size - 4).unwrap();
  let image = storage::capture(&file).unwrap();
  let root = private();
  let directory = File::open(root.path()).unwrap();
  let mut checks = 0;
  let record = storage::record::save(&image, &directory, || {
    checks += 1;
    anyhow::ensure!(checks < 100, "Sparse capture scanned its logical capacity");
    Ok(())
  })
  .unwrap();
  let mut checks = 0;
  storage::record::load(&directory, &record, size, || {
    checks += 1;
    anyhow::ensure!(checks < 100, "Sparse load scanned its logical capacity");
    Ok(())
  })
  .unwrap();
  let stage = private();
  let prepared = image
    .prepare(&File::open(stage.path()).unwrap(), || Ok(()))
    .unwrap();
  let mut cursor = prepared.file();
  cursor.seek(SeekFrom::Start(123)).unwrap();
  let mut checks = 0;
  prepared
    .validate(&image, || {
      checks += 1;
      anyhow::ensure!(
        checks < 100,
        "Prepared comparison scanned its logical capacity"
      );
      Ok(())
    })
    .unwrap();
  assert_eq!(cursor.stream_position().unwrap(), 123);
  prepared
    .file()
    .write_all_at(b"changed hole", 10 << 30)
    .unwrap();
  assert!(prepared.validate(&image, || Ok(())).is_err());
  assert_eq!(cursor.stream_position().unwrap(), 123);
  let path = root.path().join("image");
  fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
  let writer = fs::OpenOptions::new().write(true).open(&path).unwrap();
  writer.write_all_at(b"changed hole", 10 << 30).unwrap();
  fs::set_permissions(path, fs::Permissions::from_mode(0o400)).unwrap();
  assert!(storage::record::load(&directory, &record, size, || Ok(())).is_err());
}

#[test]
fn legacy_full_file_checksums_remain_readable_and_unknown_versions_reject() {
  let data = vec![0x51; 128 << 10];
  let file = tempfile::tempfile().unwrap();
  file.write_all_at(&data, 0).unwrap();
  let (mut record, root) = save(&file);
  record[..4].copy_from_slice(b"HDS1");
  record[12..44].copy_from_slice(&Sha256::digest(&data));
  let checksum = Sha256::digest(&record[..44]);
  record[44..].copy_from_slice(&checksum);
  let directory = File::open(root.path()).unwrap();
  let loaded = storage::record::load(&directory, &record, data.len() as u64, || Ok(())).unwrap();
  let mut observed = [0; 16];
  loaded.read(0, &mut observed).unwrap();
  assert_eq!(observed, [0x51; 16]);
  record[..4].copy_from_slice(b"HDS3");
  let checksum = Sha256::digest(&record[..44]);
  record[44..].copy_from_slice(&checksum);
  assert!(storage::record::load(&directory, &record, data.len() as u64, || Ok(())).is_err());
}
