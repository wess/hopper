use machine::runtime::variables::{self, SIZE};
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[test]
fn confirmed_changes_survive_reopen_and_exclusive_ownership() {
  let temp = tempfile::tempdir().unwrap();
  let root = temp.path().join("variables");
  let mut store = variables::create(&root, &[1, 2, 3, 4]).unwrap();
  assert_eq!(
    &variables::bytes(&store)[..8],
    &[1, 2, 3, 4, 255, 255, 255, 255]
  );
  assert!(variables::open(&root).is_err());
  assert!(variables::create(&root, &[0]).is_err());
  for (offset, data) in [
    (1, vec![0; 4]),
    (0, vec![]),
    (SIZE, vec![0; 4]),
    (0, vec![0; 0x40004]),
  ] {
    assert!(variables::commit(&mut store, offset, &data).is_err());
  }
  assert_eq!(fs::metadata(root.join("journal")).unwrap().len(), 0);
  variables::commit(&mut store, 4, &[5, 6, 7, 8]).unwrap();
  drop(store);
  let store = variables::open(&root).unwrap();
  assert_eq!(&variables::bytes(&store)[..8], &[1, 2, 3, 4, 5, 6, 7, 8]);
  assert_eq!(fs::metadata(root.join("journal")).unwrap().len(), 0);
  #[cfg(unix)]
  {
    assert_eq!(
      fs::metadata(&root).unwrap().permissions().mode() & 0o777,
      0o700
    );
    for name in ["bank", "journal", "lock"] {
      assert_eq!(
        fs::metadata(root.join(name)).unwrap().permissions().mode() & 0o777,
        0o600
      );
    }
  }
}

#[test]
fn incomplete_last_records_preserve_the_confirmed_prefix() {
  let temp = tempfile::tempdir().unwrap();
  let root = temp.path().join("variables");
  let mut store = variables::create(&root, &[0xff; 4]).unwrap();
  variables::commit(&mut store, 0, &[1; 4]).unwrap();
  variables::commit(&mut store, 4, &[2; 4]).unwrap();
  let journal = fs::read(root.join("journal")).unwrap();
  assert_eq!(journal.len(), 200);
  drop(store);
  for cut in [0, 1, 8, 24, 55, 56, 60, 99] {
    fs::write(root.join("journal"), &journal[..100 + cut]).unwrap();
    let store = variables::open(&root).unwrap();
    assert_eq!(
      &variables::bytes(&store)[..8],
      &[1, 1, 1, 1, 255, 255, 255, 255]
    );
    assert_eq!(fs::metadata(root.join("journal")).unwrap().len(), 0);
  }
}

#[test]
fn corrupt_complete_records_are_preserved_and_never_replayed() {
  let temp = tempfile::tempdir().unwrap();
  let root = temp.path().join("variables");
  let mut store = variables::create(&root, &[0xff; 4]).unwrap();
  variables::commit(&mut store, 0, &[1; 4]).unwrap();
  variables::commit(&mut store, 4, &[2; 4]).unwrap();
  let journal = fs::read(root.join("journal")).unwrap();
  drop(store);
  for offset in [0, 8, 16, 20, 24, 56, 60, 92, 100, 156] {
    let mut corrupt = journal.clone();
    corrupt[offset] ^= 1;
    fs::write(root.join("journal"), &corrupt).unwrap();
    assert!(variables::open(&root).is_err());
    assert_eq!(fs::read(root.join("journal")).unwrap(), corrupt);
  }
  let reordered = [&journal[100..], &journal[..100]].concat();
  fs::write(root.join("journal"), &reordered).unwrap();
  assert!(variables::open(&root).is_err());
  assert_eq!(fs::read(root.join("journal")).unwrap(), reordered);
  for sequence in [1u64, 3] {
    let mut corrupt = journal.clone();
    corrupt[108..116].copy_from_slice(&sequence.to_le_bytes());
    use sha2::{Digest, Sha256};
    let checksum = Sha256::digest(&corrupt[100..124]);
    corrupt[124..156].copy_from_slice(&checksum);
    fs::write(root.join("journal"), &corrupt).unwrap();
    assert!(variables::open(&root).is_err());
    assert_eq!(fs::read(root.join("journal")).unwrap(), corrupt);
  }
}

#[test]
fn snapshot_checksums_and_size_bounds_fail_without_replacing_state() {
  let temp = tempfile::tempdir().unwrap();
  let root = temp.path().join("variables");
  drop(variables::create(&root, &[0xff; 4]).unwrap());
  let mut bank = fs::read(root.join("bank")).unwrap();
  bank[16] ^= 1;
  fs::write(root.join("bank"), &bank).unwrap();
  assert!(variables::open(&root).is_err());
  assert_eq!(fs::read(root.join("bank")).unwrap(), bank);
  fs::write(root.join("bank"), [0]).unwrap();
  assert!(variables::open(&root).is_err());
  assert_eq!(fs::read(root.join("bank")).unwrap(), [0]);
}

#[test]
fn compaction_does_not_revert_changes_when_an_old_journal_remains() {
  let temp = tempfile::tempdir().unwrap();
  let root = temp.path().join("variables");
  let mut store = variables::create(&root, &[0xff; 4]).unwrap();
  for value in 1..=15 {
    variables::commit(&mut store, 0, &vec![value; 0x40000]).unwrap();
  }
  let old = fs::read(root.join("journal")).unwrap();
  variables::commit(&mut store, 0, &vec![16; 0x40000]).unwrap();
  assert_eq!(fs::metadata(root.join("journal")).unwrap().len(), 0);
  drop(store);
  fs::write(root.join("journal"), old).unwrap();
  let mut store = variables::open(&root).unwrap();
  assert!(variables::bytes(&store)[..0x40000]
    .iter()
    .all(|byte| *byte == 16));
  variables::commit(&mut store, 0, &[17; 4]).unwrap();
  drop(store);
  let store = variables::open(&root).unwrap();
  assert_eq!(
    &variables::bytes(&store)[..8],
    &[17, 17, 17, 17, 16, 16, 16, 16]
  );
}

#[cfg(unix)]
#[test]
fn public_files_and_symlinks_are_rejected() {
  let temp = tempfile::tempdir().unwrap();
  let root = temp.path().join("variables");
  drop(variables::create(&root, &[0xff; 4]).unwrap());
  let journal = root.join("journal");
  fs::set_permissions(&journal, fs::Permissions::from_mode(0o644)).unwrap();
  assert!(variables::open(&root).is_err());
  fs::set_permissions(&journal, fs::Permissions::from_mode(0o600)).unwrap();
  fs::rename(&journal, temp.path().join("original")).unwrap();
  std::os::unix::fs::symlink(temp.path().join("original"), &journal).unwrap();
  assert!(variables::open(&root).is_err());
  assert_eq!(fs::metadata(temp.path().join("original")).unwrap().len(), 0);
}

#[test]
fn oversized_journals_are_preserved_and_invalid_templates_create_nothing() {
  let temp = tempfile::tempdir().unwrap();
  let root = temp.path().join("variables");
  for template in [vec![], vec![0; SIZE + 1]] {
    assert!(variables::create(&root, &template).is_err());
    assert!(!root.exists());
  }
  drop(variables::create(&root, &[0xff; 4]).unwrap());
  fs::OpenOptions::new()
    .write(true)
    .open(root.join("journal"))
    .unwrap()
    .set_len(8 * 1024 * 1024)
    .unwrap();
  assert!(variables::open(&root).is_err());
  assert_eq!(
    fs::metadata(root.join("journal")).unwrap().len(),
    8 * 1024 * 1024
  );
}
