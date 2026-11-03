#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use fs2::FileExt;
use machine::snapshot::{
  manifest::{Binding, Layout},
  package::{Package, Stage},
};
use sha2::{Digest, Sha256};
use std::{
  cell::Cell,
  fs::{File, OpenOptions, Permissions},
  os::unix::fs::PermissionsExt,
  path::Path,
  process::Command,
};

struct Guard(File);
impl Drop for Guard {
  fn drop(&mut self) {
    FileExt::unlock(&self.0).unwrap();
  }
}

fn lock(path: &Path) -> anyhow::Result<Guard> {
  let file = OpenOptions::new().read(true).write(true).open(path)?;
  file.try_lock_exclusive()?;
  Ok(Guard(file))
}

fn binding() -> Binding {
  Binding {
    machine: [1; 16],
    snapshot: [2; 16],
    configuration: [3; 32],
  }
}

fn layout() -> Layout {
  Layout {
    cpus: 1,
    banks: [(0, 65536), (65536, 65536), (1 << 30, 65536)],
    devices: [0; 7],
  }
}

fn stage(root: &Path) -> Stage {
  std::fs::set_permissions(root, Permissions::from_mode(0o700)).unwrap();
  let owner = File::open(root).unwrap();
  let mut stage = Stage::new(&owner, binding(), layout(), || Ok(())).unwrap();
  let mut bytes = b"test component".to_vec();
  bytes.extend_from_slice(&Sha256::digest(&bytes));
  for record in layout().records().unwrap() {
    stage
      .write(record, || Ok(()), |out| Ok(out.write_all(&bytes)?))
      .unwrap();
  }
  stage
}

#[test]
fn commit_guard_excludes_policy_changes_through_rename_and_final_check() {
  for fail_after_rename in [false, true] {
    let root = tempfile::tempdir().unwrap();
    let stage = stage(root.path());
    let path = root.path().join("access");
    File::create(&path).unwrap();
    let entered = Cell::new(false);
    let checks = Cell::new(0);
    let destination = root.path().join("02".repeat(16));
    let result = stage.publish(
      || {
        checks.set(checks.get() + 1);
        if entered.get() {
          // independent opens contend with the same filesystem lock used by policy changes.
          assert!(lock(&path).is_err());
          if destination.exists() {
            assert_eq!(checks.get(), 3);
            let status = Command::new(std::env::current_exe()?)
              .args(["--exact", "policy_change_process", "--ignored"])
              .env("HOPPER_COMMIT_LOCK", &path)
              .status()?;
            assert!(status.success());
            assert!(lock(&path).is_err());
            anyhow::ensure!(!fail_after_rename, "cancelled after rename");
          } else {
            assert_eq!(checks.get(), 2);
          }
        } else {
          assert_eq!(checks.get(), 1);
          drop(lock(&path)?);
        }
        Ok(())
      },
      || {
        let guard = lock(&path)?;
        entered.set(true);
        Ok(guard)
      },
    );
    assert_eq!(result.is_err(), fail_after_rename);
    assert_eq!(checks.get(), 3);
    drop(lock(&path).unwrap());
    // an error after publication preserves the complete package without retaining the lock.
    assert!(Package::open(
      &File::open(root.path()).unwrap(),
      binding(),
      layout(),
      || Ok(())
    )
    .is_ok());
  }
}

#[test]
fn policy_changed_before_guard_acquisition_rejects_before_rename() {
  let root = tempfile::tempdir().unwrap();
  let stage = stage(root.path());
  let path = root.path().join("access");
  File::create(&path).unwrap();
  let generation = Cell::new(4);
  let checks = Cell::new(0);
  let result = stage.publish(
    || {
      checks.set(checks.get() + 1);
      anyhow::ensure!(generation.get() == 4, "original policy revoked");
      Ok(())
    },
    || {
      // an off/on toggle between periodic authorization and commit must not revive this request.
      generation.set(6);
      lock(&path)
    },
  );
  assert!(result
    .err()
    .unwrap()
    .to_string()
    .contains("original policy revoked"));
  assert_eq!(checks.get(), 2);
  assert!(!root.path().join("02".repeat(16)).exists());
  drop(lock(&path).unwrap());
  assert_eq!(
    std::fs::read_dir(root.path())
      .unwrap()
      .filter_map(Result::ok)
      .filter(|entry| entry.file_name().to_string_lossy().starts_with(".stage"))
      .count(),
    1
  );
}

#[test]
fn failed_commit_lease_cannot_publish_or_replace() {
  let root = tempfile::tempdir().unwrap();
  let stage = stage(root.path());
  let path = root.path().join("access");
  File::create(&path).unwrap();
  let busy = lock(&path).unwrap();
  assert!(stage.publish(|| Ok(()), || lock(&path)).is_err());
  assert!(!root.path().join("02".repeat(16)).exists());
  drop(busy);
  drop(lock(&path).unwrap());
}

#[test]
fn duplicate_destination_error_releases_commit_lease() {
  let root = tempfile::tempdir().unwrap();
  let path = root.path().join("access");
  File::create(&path).unwrap();
  drop(
    stage(root.path())
      .publish(|| Ok(()), || lock(&path))
      .unwrap(),
  );
  let before = std::fs::read(root.path().join("02".repeat(16)).join("manifest.hsm")).unwrap();
  assert!(stage(root.path())
    .publish(|| Ok(()), || lock(&path))
    .is_err());
  drop(lock(&path).unwrap());
  assert_eq!(
    std::fs::read(root.path().join("02".repeat(16)).join("manifest.hsm")).unwrap(),
    before
  );
}

#[test]
#[ignore = "invoked under the writer commit guard in a separate process"]
fn policy_change_process() {
  let path = std::env::var_os("HOPPER_COMMIT_LOCK").unwrap();
  assert!(lock(Path::new(&path)).is_err());
}
