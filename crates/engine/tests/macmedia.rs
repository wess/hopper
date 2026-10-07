#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use engine::machines::{
  vz::{self, mac::media, records, Service},
  Actor, Machines,
};
use machine::vz::queue::Check;
use std::{
  os::unix::fs::PermissionsExt,
  sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
  },
};

fn fixture() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
  let root = tempfile::tempdir().unwrap();
  let image = root.path().join("source.ipsw");
  std::fs::write(&image, b"synthetic restore media").unwrap();
  let parent = root.path().join("staging");
  std::fs::create_dir(&parent).unwrap();
  std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o700)).unwrap();
  (root, image, parent)
}

#[test]
fn private_restore_staging_is_independent_and_removes_only_its_owned_copy() {
  let (_root, image, parent) = fixture();
  let check: Check = Arc::new(|| Ok(()));
  let staged = media::stage(&image, &parent, &check).unwrap();
  let copy = staged.path().join("restore.ipsw");
  assert_eq!(std::fs::read(&copy).unwrap(), b"synthetic restore media");
  assert_eq!(
    std::fs::metadata(&copy).unwrap().permissions().mode() & 0o777,
    0o600
  );
  assert_eq!(
    std::fs::metadata(staged.path())
      .unwrap()
      .permissions()
      .mode()
      & 0o777,
    0o700
  );
  std::fs::write(&image, b"changed source").unwrap();
  assert_eq!(std::fs::read(&copy).unwrap(), b"synthetic restore media");
  std::fs::write(&copy, b"changed staged copy").unwrap();
  assert_eq!(std::fs::read(&image).unwrap(), b"changed source");
  drop(staged);
  assert!(!copy.exists());
  assert_eq!(std::fs::read_dir(parent).unwrap().count(), 0);
  assert_eq!(std::fs::read(image).unwrap(), b"changed source");
}

#[test]
fn cancelled_staging_cleans_temporary_state_without_changing_source() {
  let (_root, image, parent) = fixture();
  let count = Arc::new(AtomicUsize::new(0));
  let calls = count.clone();
  let check: Check = Arc::new(move || {
    anyhow::ensure!(calls.fetch_add(1, Ordering::SeqCst) < 2, "cancelled");
    Ok(())
  });
  assert!(media::stage(&image, &parent, &check).is_err());
  assert!(count.load(Ordering::SeqCst) >= 3);
  assert_eq!(std::fs::read_dir(parent).unwrap().count(), 0);
  assert_eq!(std::fs::read(image).unwrap(), b"synthetic restore media");
}

#[test]
fn invalid_restore_sources_and_readable_staging_are_rejected_without_publication() {
  let (root, image, parent) = fixture();
  let check: Check = Arc::new(|| Ok(()));
  let linked = root.path().join("linked.ipsw");
  std::os::unix::fs::symlink(&image, &linked).unwrap();
  let empty = root.path().join("empty.ipsw");
  std::fs::write(&empty, []).unwrap();
  for path in [
    &linked,
    &empty,
    root.path(),
    std::path::Path::new("relative.ipsw"),
  ] {
    assert!(media::stage(path, &parent, &check).is_err());
  }
  assert_eq!(std::fs::read_dir(&parent).unwrap().count(), 0);
  std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o755)).unwrap();
  assert!(media::stage(&image, &parent, &check).is_err());
  assert_eq!(std::fs::read(image).unwrap(), b"synthetic restore media");
}

#[tokio::test]
async fn revoked_mac_inspection_and_absent_media_fail_before_staging() {
  let (root, image, _parent) = fixture();
  let manager = Machines {
    root: root.path().join("machines"),
  };
  let machine = records::create(
    &manager,
    model::CreateMachine {
      name: "Inspection fixture".into(),
      profile: "macos".into(),
      resources: model::EngineResources {
        cpus: 2,
        memory_gib: 4,
        disk_gib: 64,
      },
      agent_access: false,
      installer: Some(image.to_str().unwrap().into()),
    },
  )
  .unwrap();
  let (client, _) = vz::channel();
  let service = Service::new(manager.clone(), client);
  assert!(service
    .inspect_mac(&machine.id, Actor::Agent)
    .await
    .is_err());
  assert!(!manager.root.join("vz").exists());
  let machine = records::create(
    &manager,
    model::CreateMachine {
      name: "Missing media fixture".into(),
      profile: "macos".into(),
      resources: model::EngineResources {
        cpus: 2,
        memory_gib: 4,
        disk_gib: 64,
      },
      agent_access: true,
      installer: None,
    },
  )
  .unwrap();
  let error = service
    .inspect_mac(&machine.id, Actor::Person)
    .await
    .err()
    .unwrap();
  assert!(error.to_string().contains("Acquire macOS restore media"));
  assert!(!manager.root.join("vz").exists());
  assert_eq!(std::fs::read(image).unwrap(), b"synthetic restore media");
}

#[tokio::test]
async fn explicit_mac_stop_records_cancellation_even_during_an_owned_operation() {
  let (root, image, _parent) = fixture();
  let manager = Machines {
    root: root.path().join("machines"),
  };
  let machine = records::create(
    &manager,
    model::CreateMachine {
      name: "Cancellation fixture".into(),
      profile: "macos".into(),
      resources: model::EngineResources {
        cpus: 2,
        memory_gib: 4,
        disk_gib: 64,
      },
      agent_access: true,
      installer: Some(image.to_str().unwrap().into()),
    },
  )
  .unwrap();
  let operation = std::fs::OpenOptions::new()
    .read(true)
    .write(true)
    .open(manager.root.join("locks").join(&machine.id))
    .unwrap();
  let _held = store::lock::exclusive(operation).unwrap();
  let (client, _) = vz::channel();
  let service = Service::new(manager.clone(), client);
  let intent = manager.root.join("intent").join(&machine.id);
  assert!(service
    .transition(&machine.id, Actor::Person, vz::Action::Stop)
    .await
    .is_err());
  let first = std::fs::read(&intent).unwrap();
  let value: serde_json::Value = serde_json::from_slice(&first).unwrap();
  assert_eq!(value["version"], 1);
  assert!(uuid::Uuid::parse_str(value["stop"].as_str().unwrap()).is_ok());
  assert_eq!(
    std::fs::metadata(&intent).unwrap().permissions().mode() & 0o777,
    0o600
  );
  manager.set_agent_access(&machine.id, false).unwrap();
  assert!(service
    .transition(&machine.id, Actor::Agent, vz::Action::Stop)
    .await
    .is_err());
  assert_eq!(std::fs::read(&intent).unwrap(), first);
  assert!(service
    .transition(&machine.id, Actor::Person, vz::Action::Stop)
    .await
    .is_err());
  assert_ne!(std::fs::read(&intent).unwrap(), first);
  assert_eq!(std::fs::read(image).unwrap(), b"synthetic restore media");
}
