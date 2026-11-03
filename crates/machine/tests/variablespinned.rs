#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
use machine::runtime::variables;
use std::{
  fs::{self, File, Permissions},
  os::unix::fs::{symlink, PermissionsExt},
};

#[test]
fn renamed_stage_and_replaced_paths_cannot_redirect_compaction() {
  let temp = tempfile::tempdir().unwrap();
  let stage = temp.path().join("stage");
  fs::create_dir(&stage).unwrap();
  fs::set_permissions(&stage, Permissions::from_mode(0o700)).unwrap();
  let parent = File::open(&stage).unwrap();
  let mut store = variables::create_at(&parent, &[1; 4]).unwrap();
  assert!(variables::open_at(&parent).is_err());
  let published = temp.path().join("published");
  fs::rename(&stage, &published).unwrap();
  fs::create_dir(&stage).unwrap();
  fs::set_permissions(&stage, Permissions::from_mode(0o700)).unwrap();
  let replacement = File::open(&stage).unwrap();
  drop(variables::create_at(&replacement, &[2; 4]).unwrap());
  // move the child as well: compaction must use the original inode, not either old path.
  let retained = published.join("retained");
  fs::rename(published.join("variables"), &retained).unwrap();
  drop(variables::create_at(&parent, &[3; 4]).unwrap());
  for value in 4..=19 {
    variables::commit(&mut store, 0, &vec![value; 0x40000]).unwrap();
  }
  assert_eq!(fs::metadata(retained.join("journal")).unwrap().len(), 0);
  drop(store);
  let restored = variables::open(&retained).unwrap();
  assert!(variables::bytes(&restored)[..0x40000]
    .iter()
    .all(|byte| *byte == 19));
  assert_eq!(
    &variables::bytes(&variables::open_at(&parent).unwrap())[..4],
    &[3; 4]
  );
  assert_eq!(
    &variables::bytes(&variables::open_at(&replacement).unwrap())[..4],
    &[2; 4]
  );
}

#[test]
fn relative_creation_preserves_existing_state_and_rejects_unsafe_namespaces() {
  let root = tempfile::tempdir().unwrap();
  fs::set_permissions(root.path(), Permissions::from_mode(0o700)).unwrap();
  let parent = File::open(root.path()).unwrap();
  assert!(variables::create_at(&parent, &[]).is_err());
  assert!(!root.path().join("variables").exists());
  let outside = tempfile::tempdir().unwrap();
  symlink(outside.path(), root.path().join("variables")).unwrap();
  assert!(variables::create_at(&parent, &[1]).is_err());
  assert!(variables::open_at(&parent).is_err());
  assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
  fs::remove_file(root.path().join("variables")).unwrap();
  drop(variables::create_at(&parent, &[1; 4]).unwrap());
  assert!(variables::create_at(&parent, &[2; 4]).is_err());
  let journal = root.path().join("variables/journal");
  fs::hard_link(&journal, root.path().join("alias")).unwrap();
  assert!(variables::open_at(&parent).is_err());
  fs::remove_file(root.path().join("alias")).unwrap();
  assert_eq!(
    &variables::bytes(&variables::open_at(&parent).unwrap())[..4],
    &[1; 4]
  );
  fs::set_permissions(root.path(), Permissions::from_mode(0o755)).unwrap();
  assert!(variables::open_at(&parent).is_err());
}
