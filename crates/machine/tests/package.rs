#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use machine::snapshot::{
  manifest::{Binding, Layout, Record},
  package::{Package, Stage},
};
use sha2::{Digest, Sha256};
use std::{
  cell::Cell,
  fs::{File, Permissions},
  io::Read,
  os::unix::fs::{MetadataExt, PermissionsExt},
  path::Path,
  process::Command,
};

fn binding() -> Binding {
  Binding {
    machine: [1; 16],
    snapshot: [2; 16],
    configuration: [3; 32],
  }
}

fn layout() -> Layout {
  Layout {
    cpus: 2,
    banks: [(0, 65536), (65536, 65536), (1 << 30, 65536)],
    devices: [0; 7],
  }
}

fn parent(path: &Path) -> File {
  std::fs::set_permissions(path, Permissions::from_mode(0o700)).unwrap();
  File::open(path).unwrap()
}

// the package layer checks framing/checksums; real component schemas are tested by recovery.
fn component() -> Vec<u8> {
  let mut bytes = b"test component".to_vec();
  bytes.extend_from_slice(&Sha256::digest(&bytes));
  bytes
}

fn fill(stage: &mut Stage) {
  for record in layout().records().unwrap() {
    stage
      .write(record, || Ok(()), |out| Ok(out.write_all(&component())?))
      .unwrap();
  }
}

fn verify(package: &Package) {
  for record in layout().records().unwrap() {
    let mut reader = package.reader(record, || Ok(())).unwrap();
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).unwrap();
    reader.finish().unwrap();
    assert_eq!(bytes, component());
  }
}

#[test]
fn exclusive_publication_survives_writer_exit() {
  let root = tempfile::tempdir().unwrap();
  let owner = parent(root.path());
  let status = Command::new(std::env::current_exe().unwrap())
    .args(["--exact", "writer_process", "--ignored"])
    .env("HOPPER_PACKAGE_TEST", root.path())
    .status()
    .unwrap();
  assert!(status.success());
  let package = Package::open(&owner, binding(), layout(), || Ok(())).unwrap();
  verify(&package);
  let mut duplicate = Stage::new(&owner, binding(), layout(), || Ok(())).unwrap();
  fill(&mut duplicate);
  assert!(duplicate.publish(|| Ok(()), || Ok(())).is_err());
  verify(&package);
  let destination = root.path().join("02".repeat(16));
  assert_eq!(
    std::fs::metadata(destination.join("manifest.hsm"))
      .unwrap()
      .mode()
      & 0o7777,
    0o400
  );
  let mut foreign = binding();
  foreign.machine = [4; 16];
  assert!(Package::open(&owner, foreign, layout(), || Ok(())).is_err());
}

#[test]
#[ignore = "invoked in a separate process"]
fn writer_process() {
  let path = std::env::var_os("HOPPER_PACKAGE_TEST").unwrap();
  let owner = parent(Path::new(&path));
  let mut stage = Stage::new(&owner, binding(), layout(), || Ok(())).unwrap();
  fill(&mut stage);
  verify(&stage.publish(|| Ok(()), || Ok(())).unwrap());
}

#[test]
fn incomplete_corrupt_and_failed_encoder_stages_cannot_publish() {
  for failure in 0..4 {
    let root = tempfile::tempdir().unwrap();
    let owner = parent(root.path());
    let mut stage = Stage::new(&owner, binding(), layout(), || Ok(())).unwrap();
    if failure == 0 {
      assert!(stage.publish(|| Ok(()), || Ok(())).is_err());
    } else {
      let result = stage.write(
        Record::Cpu(0),
        || Ok(()),
        |out| {
          if failure == 1 {
            out.write_all(&[0; 46])?;
          }
          if failure == 2 {
            out.write_all(&component())?;
            anyhow::bail!("encoder failed after footer");
          }
          if failure == 3 {
            // ignoring a bounded writer error must not allow a later successful return.
            let _ = out.write_all(&vec![0; 9000]);
          }
          Ok(())
        },
      );
      assert!(result.is_err());
      assert!(stage
        .write(
          Record::Cpu(1),
          || Ok(()),
          |out| Ok(out.write_all(&component())?)
        )
        .is_err());
      assert!(stage.publish(|| Ok(()), || Ok(())).is_err());
    }
    assert!(Package::open(&owner, binding(), layout(), || Ok(())).is_err());
  }
}

#[test]
fn every_publication_authorization_failure_is_explicit_and_never_replaces() {
  for deny in 0..4 {
    let root = tempfile::tempdir().unwrap();
    let owner = parent(root.path());
    let mut stage = Stage::new(&owner, binding(), layout(), || Ok(())).unwrap();
    fill(&mut stage);
    let checks = Cell::new(0);
    let result = stage.publish(
      || {
        let current = checks.get();
        checks.set(current + 1);
        anyhow::ensure!(current != deny, "revoked");
        Ok(())
      },
      || Ok(()),
    );
    if deny < 3 {
      assert!(result.is_err());
    } else {
      assert!(result.is_ok());
    }
    if deny == 2 {
      // revocation after rename reports failure, but the complete package remains recoverable.
      verify(&Package::open(&owner, binding(), layout(), || Ok(())).unwrap());
    } else if deny < 2 {
      assert!(Package::open(&owner, binding(), layout(), || Ok(())).is_err());
    }
  }
}

#[test]
fn pinned_parent_replacement_cannot_redirect_publication_or_reads() {
  let root = tempfile::tempdir().unwrap();
  let path = root.path().join("snapshots");
  std::fs::create_dir(&path).unwrap();
  let owner = parent(&path);
  let mut stage = Stage::new(&owner, binding(), layout(), || Ok(())).unwrap();
  fill(&mut stage);
  let old = root.path().join("original");
  std::fs::rename(&path, &old).unwrap();
  std::fs::create_dir(&path).unwrap();
  let replacement = parent(&path);
  verify(&stage.publish(|| Ok(()), || Ok(())).unwrap());
  verify(&Package::open(&owner, binding(), layout(), || Ok(())).unwrap());
  assert!(Package::open(&replacement, binding(), layout(), || Ok(())).is_err());
  assert!(old.join("02".repeat(16)).exists());
}

#[test]
fn unsafe_component_files_and_foreign_directories_reject() {
  let root = tempfile::tempdir().unwrap();
  let owner = parent(root.path());
  let mut stage = Stage::new(&owner, binding(), layout(), || Ok(())).unwrap();
  fill(&mut stage);
  let package = stage.publish(|| Ok(()), || Ok(())).unwrap();
  let directory = root.path().join("02".repeat(16));
  let file = directory.join("0.hcp");
  std::fs::set_permissions(&file, Permissions::from_mode(0o600)).unwrap();
  assert!(package.reader(Record::Cpu(0), || Ok(())).is_err());
  std::fs::set_permissions(&file, Permissions::from_mode(0o400)).unwrap();
  std::fs::hard_link(&file, directory.join("extra")).unwrap();
  assert!(package.reader(Record::Cpu(0), || Ok(())).is_err());
  std::fs::remove_file(&file).unwrap();
  std::os::unix::fs::symlink(directory.join("extra"), &file).unwrap();
  assert!(package.reader(Record::Cpu(0), || Ok(())).is_err());
  std::fs::set_permissions(root.path(), Permissions::from_mode(0o755)).unwrap();
  assert!(Stage::new(&owner, binding(), layout(), || Ok(())).is_err());
  assert!(Package::open(&owner, binding(), layout(), || Ok(())).is_err());
}

#[test]
fn fragmented_large_writes_and_each_write_revocation_are_bounded() {
  for fragment in [1, 7, 31, 33, 65536] {
    let root = tempfile::tempdir().unwrap();
    let owner = parent(root.path());
    let mut stage = Stage::new(&owner, binding(), layout(), || Ok(())).unwrap();
    let mut bytes = vec![9; 65560];
    bytes.extend_from_slice(&Sha256::digest(&bytes));
    for record in layout().records().unwrap() {
      stage
        .write(
          record,
          || Ok(()),
          |out| {
            let data = if record == Record::Memory(0) {
              &bytes
            } else {
              &component()
            };
            for chunk in data.chunks(fragment) {
              out.write_all(chunk)?;
            }
            Ok(())
          },
        )
        .unwrap();
    }
    let package = stage.publish(|| Ok(()), || Ok(())).unwrap();
    let mut reader = package.reader(Record::Memory(0), || Ok(())).unwrap();
    let mut reopened = Vec::new();
    reader.read_to_end(&mut reopened).unwrap();
    reader.finish().unwrap();
    assert_eq!(bytes, reopened);
  }
  for deny in 0..5 {
    let root = tempfile::tempdir().unwrap();
    let owner = parent(root.path());
    let mut stage = Stage::new(&owner, binding(), layout(), || Ok(())).unwrap();
    let checks = Cell::new(0);
    assert!(stage
      .write(
        Record::Cpu(0),
        || {
          let current = checks.get();
          checks.set(current + 1);
          anyhow::ensure!(current != deny, "revoked");
          Ok(())
        },
        |out| Ok(out.write_all(&component())?)
      )
      .is_err());
    assert!(stage.publish(|| Ok(()), || Ok(())).is_err());
    assert!(Package::open(&owner, binding(), layout(), || Ok(())).is_err());
  }
}

#[test]
fn replaced_stage_is_rejected_without_publishing_the_replacement() {
  let root = tempfile::tempdir().unwrap();
  let owner = parent(root.path());
  let mut stage = Stage::new(&owner, binding(), layout(), || Ok(())).unwrap();
  fill(&mut stage);
  let name = std::fs::read_dir(root.path())
    .unwrap()
    .next()
    .unwrap()
    .unwrap()
    .path();
  std::fs::rename(&name, root.path().join("retained")).unwrap();
  std::fs::create_dir(&name).unwrap();
  parent(&name);
  assert!(stage.publish(|| Ok(()), || Ok(())).is_err());
  assert!(Package::open(&owner, binding(), layout(), || Ok(())).is_err());
}
