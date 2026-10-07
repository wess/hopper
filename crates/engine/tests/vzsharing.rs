#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use engine::machines::{
  vz::{records, sharing, Service, Stage},
  Actor, Machines,
};
use fs2::FileExt;
use model::MachineFolder;
use std::{
  fs::OpenOptions,
  os::unix::fs::{MetadataExt, PermissionsExt},
};

fn fixture(profile: &str) -> (tempfile::TempDir, Machines, model::Machine, MachineFolder) {
  let root = tempfile::tempdir().unwrap();
  let manager = Machines {
    root: root.path().join("machines"),
  };
  let iso = root.path().join("installer.iso");
  let mut data = vec![0; 32775];
  data[32769..32774].copy_from_slice(b"CD001");
  std::fs::write(&iso, data).unwrap();
  let machine = records::create(
    &manager,
    model::CreateMachine {
      name: "Folder fixture".into(),
      profile: profile.into(),
      resources: model::EngineResources {
        cpus: 2,
        memory_gib: 4,
        disk_gib: 64,
      },
      installer: Some(iso.to_str().unwrap().into()),
      agent_access: true,
    },
  )
  .unwrap();
  let path = root.path().join("authorized");
  std::fs::create_dir(&path).unwrap();
  let folder = MachineFolder {
    name: "work".into(),
    path: path.to_str().unwrap().into(),
    read_only: true,
  };
  (root, manager, machine, folder)
}

#[test]
fn private_grants_preserve_identity_modes_and_can_remove_missing_directories() {
  for profile in ["ubuntu", "macos"] {
    let (_root, manager, machine, folder) = fixture(profile);
    let id = &machine.id;
    assert!(sharing::folders(&manager, id).unwrap().is_empty());
    sharing::add(&manager, id, folder.clone()).unwrap();
    let directory = manager.root.join("sharing");
    let path = directory.join(id);
    assert_eq!(std::fs::metadata(&directory).unwrap().mode() & 0o777, 0o700);
    assert_eq!(std::fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
    let reopened = Machines {
      root: manager.root.clone(),
    };
    let saved = sharing::folders(&reopened, id).unwrap();
    assert_eq!(saved[0].name, folder.name);
    assert_eq!(
      std::path::Path::new(&saved[0].path),
      std::path::Path::new(&folder.path).canonicalize().unwrap()
    );
    assert!(saved[0].read_only);
    sharing::set_read_only(&manager, id, "work", false).unwrap();
    assert!(!sharing::folders(&reopened, id).unwrap()[0].read_only);
    assert!(sharing::add(&manager, id, folder.clone()).is_err());
    std::fs::remove_dir(&folder.path).unwrap();
    assert_eq!(sharing::folders(&reopened, id).unwrap().len(), 1);
    assert!(sharing::set_read_only(&manager, id, "work", true).is_err());
    sharing::remove(&manager, id, "work").unwrap();
    assert!(sharing::folders(&reopened, id).unwrap().is_empty());
    assert!(!manager.root.join("vz").exists());
  }
}

#[test]
fn active_operations_and_hardware_exclude_all_grant_mutations() {
  let (_root, manager, machine, folder) = fixture("ubuntu");
  let id = &machine.id;
  sharing::add(&manager, id, folder.clone()).unwrap();
  let policy = manager.root.join("sharing").join(id);
  let original = std::fs::read(&policy).unwrap();
  for suffix in ["", ".runtime"] {
    let lock = OpenOptions::new()
      .read(true)
      .write(true)
      .create(true)
      .truncate(false)
      .open(manager.root.join("locks").join(format!("{id}{suffix}")))
      .unwrap();
    lock.try_lock_exclusive().unwrap();
    assert!(sharing::add(
      &manager,
      id,
      MachineFolder {
        name: "other".into(),
        ..folder.clone()
      }
    )
    .is_err());
    assert!(sharing::remove(&manager, id, "work").is_err());
    assert!(sharing::set_read_only(&manager, id, "work", false).is_err());
    assert_eq!(std::fs::read(&policy).unwrap(), original);
    FileExt::unlock(&lock).unwrap();
  }
}

#[tokio::test]
async fn replaced_grants_reject_preparation_before_disk_allocation_and_require_reselection() {
  let (root, manager, machine, folder) = fixture("ubuntu");
  let id = &machine.id;
  sharing::add(&manager, id, folder.clone()).unwrap();
  std::fs::rename(&folder.path, root.path().join("original")).unwrap();
  std::fs::create_dir(&folder.path).unwrap();
  let (client, _owner) = engine::machines::vz::channel();
  let service = Service::new(manager.clone(), client);
  assert!(service
    .prepare_linux(id, Actor::Person, Stage::Installer)
    .await
    .is_err());
  assert!(!manager.root.join("vz").exists());
  assert!(sharing::set_read_only(&manager, id, "work", false).is_err());
  sharing::remove(&manager, id, "work").unwrap();
  sharing::add(&manager, id, folder).unwrap();
  let prepared = service
    .prepare_linux(id, Actor::Person, Stage::Installer)
    .await
    .unwrap();
  assert!(sharing::remove(&manager, id, "work").is_err());
  drop(prepared);
  sharing::remove(&manager, id, "work").unwrap();
}

#[test]
fn corrupt_foreign_public_symlinked_and_oversized_grants_are_preserved() {
  let (_root, manager, machine, folder) = fixture("macos");
  let id = &machine.id;
  sharing::add(&manager, id, folder.clone()).unwrap();
  let path = manager.root.join("sharing").join(id);
  for data in [
    b"broken".to_vec(),
    serde_json::to_vec(&serde_json::json!({"version":2,"id":id,"entries":[]})).unwrap(),
    serde_json::to_vec(&serde_json::json!({"version":1,"id":model::new_uuid(),"entries":[]}))
      .unwrap(),
    vec![b' '; (128 << 10) + 1],
  ] {
    std::fs::write(&path, &data).unwrap();
    assert!(sharing::folders(&manager, id).is_err());
    assert!(sharing::remove(&manager, id, "work").is_err());
    assert!(sharing::add(&manager, id, folder.clone()).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), data);
  }
  std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
  assert!(sharing::folders(&manager, id).is_err());
  std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
  let outside = manager.root.join("outside");
  std::fs::rename(&path, &outside).unwrap();
  std::os::unix::fs::symlink(&outside, &path).unwrap();
  assert!(sharing::folders(&manager, id).is_err());
  assert!(sharing::remove(&manager, id, "work").is_err());
  assert!(path.is_symlink());
}

#[test]
fn limits_and_legacy_runtime_checks_preserve_existing_grants() {
  let (_root, manager, mut machine, folder) = fixture("ubuntu");
  let id = machine.id.clone();
  for index in 0..16 {
    sharing::add(
      &manager,
      &id,
      MachineFolder {
        name: format!("work{index}"),
        ..folder.clone()
      },
    )
    .unwrap();
  }
  let path = manager.root.join("sharing").join(&id);
  let original = std::fs::read(&path).unwrap();
  assert!(sharing::add(&manager, &id, folder).is_err());
  let legacy = manager.root.join("lima").join(&id);
  std::fs::create_dir_all(&legacy).unwrap();
  std::fs::write(legacy.join("disk"), b"preserved").unwrap();
  assert!(sharing::remove(&manager, &id, "work0").is_err());
  assert_eq!(std::fs::read(legacy.join("disk")).unwrap(), b"preserved");
  std::fs::remove_dir_all(&legacy).unwrap();
  for runtime in [None, Some(model::MachineRuntime::Hypervisor)] {
    machine.runtime = runtime;
    machine.guest = model::GuestOs::Windows;
    store::json::write(
      &manager.root.join("records").join(format!("{id}.json")),
      &machine,
    )
    .unwrap();
    assert!(sharing::folders(&manager, &id).is_err());
    assert!(sharing::remove(&manager, &id, "work0").is_err());
    assert_eq!(std::fs::read(&path).unwrap(), original);
  }
}

#[test]
fn mac_preparation_freezes_grants_and_rejects_replaced_directories_before_allocation() {
  let (root, manager, machine, folder) = fixture("macos");
  sharing::add(&manager, &machine.id, folder.clone()).unwrap();
  let image = machine::vz::restore::Image {
    url: String::new(),
    build: "fixture".into(),
    version: [26, 0, 0],
    hardware: vec![1],
    minimum_cpus: 2,
    minimum_memory: 4 << 30,
  };
  let check: machine::vz::queue::Check = std::sync::Arc::new(|| Ok(()));
  std::fs::rename(&folder.path, root.path().join("original")).unwrap();
  std::fs::create_dir(&folder.path).unwrap();
  assert!(engine::machines::vz::mac::platform::prepare(
    &manager,
    machine.clone(),
    image.clone(),
    check.clone()
  )
  .is_err());
  assert!(!manager.root.join("vz").exists());
  sharing::remove(&manager, &machine.id, "work").unwrap();
  sharing::add(&manager, &machine.id, folder).unwrap();
  let prepared =
    engine::machines::vz::mac::platform::prepare(&manager, machine.clone(), image, check).unwrap();
  assert!(sharing::remove(&manager, &machine.id, "work").is_err());
  drop(prepared);
  sharing::remove(&manager, &machine.id, "work").unwrap();
}
