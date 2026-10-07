#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use engine::machines::{
  vz::{network, records},
  Actor, Machines,
};
use fs2::FileExt;
use std::{fs::OpenOptions, os::unix::fs::MetadataExt};

fn fixture(profile: &str) -> (tempfile::TempDir, Machines, model::Machine) {
  let root = tempfile::tempdir().unwrap();
  let manager = Machines {
    root: root.path().join("machines"),
  };
  let machine = records::create(
    &manager,
    model::CreateMachine {
      name: "Network fixture".into(),
      profile: profile.into(),
      resources: model::EngineResources {
        cpus: 2,
        memory_gib: 4,
        disk_gib: 64,
      },
      installer: None,
      agent_access: true,
    },
  )
  .unwrap();
  (root, manager, machine)
}

#[test]
fn default_nat_and_private_offline_policy_survive_reopening_without_allocating_hardware() {
  for profile in ["ubuntu", "macos"] {
    let (_root, manager, machine) = fixture(profile);
    let id = &machine.id;
    let record = manager.root.join("records").join(format!("{id}.json"));
    let original = std::fs::read(&record).unwrap();
    assert!(network::connected(&manager, id, Actor::Person).unwrap());
    assert!(!manager.root.join("network").exists());
    network::set_connected(&manager, id, false).unwrap();
    let reopened = Machines {
      root: manager.root.clone(),
    };
    assert!(!network::connected(&reopened, id, Actor::Agent).unwrap());
    let directory = manager.root.join("network");
    assert_eq!(std::fs::metadata(&directory).unwrap().mode() & 0o777, 0o700);
    assert_eq!(
      std::fs::metadata(directory.join(id)).unwrap().mode() & 0o777,
      0o600
    );
    network::set_connected(&manager, id, true).unwrap();
    assert!(network::connected(&reopened, id, Actor::Person).unwrap());
    assert_eq!(std::fs::read(record).unwrap(), original);
    assert!(!manager.root.join("vz").exists());
    manager.set_agent_access(id, false).unwrap();
    assert!(network::connected(&manager, id, Actor::Agent).is_err());
  }
}

#[test]
fn active_operation_and_runtime_ownership_exclude_network_changes() {
  let (_root, manager, machine) = fixture("ubuntu");
  for suffix in ["", ".runtime"] {
    let path = manager
      .root
      .join("locks")
      .join(format!("{}{suffix}", machine.id));
    let lock = OpenOptions::new()
      .read(true)
      .write(true)
      .create(true)
      .truncate(false)
      .open(path)
      .unwrap();
    lock.try_lock_exclusive().unwrap();
    assert!(network::set_connected(&manager, &machine.id, false).is_err());
    assert!(network::connected(&manager, &machine.id, Actor::Person).unwrap());
    assert!(!manager.root.join("network").exists());
    FileExt::unlock(&lock).unwrap();
  }
  network::set_connected(&manager, &machine.id, false).unwrap();
}

#[test]
fn invalid_and_foreign_offline_settings_never_reconnect_or_get_replaced() {
  let (_root, manager, machine) = fixture("macos");
  let id = &machine.id;
  network::set_connected(&manager, id, false).unwrap();
  let path = manager.root.join("network").join(id);
  for data in [
    b"broken".to_vec(),
    serde_json::to_vec(&serde_json::json!({"version":2,"id":id,"connected":false})).unwrap(),
    serde_json::to_vec(&serde_json::json!({"version":1,"id":model::new_uuid(),"connected":false}))
      .unwrap(),
    vec![b' '; 1025],
  ] {
    std::fs::write(&path, &data).unwrap();
    assert!(network::connected(&manager, id, Actor::Person).is_err());
    assert!(network::set_connected(&manager, id, true).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), data);
  }
  let outside = manager.root.join("outside");
  std::fs::rename(&path, &outside).unwrap();
  std::os::unix::fs::symlink(&outside, &path).unwrap();
  assert!(network::connected(&manager, id, Actor::Person).is_err());
  assert!(network::set_connected(&manager, id, true).is_err());
  assert!(path.is_symlink());
  assert_eq!(std::fs::read(outside).unwrap(), vec![b' '; 1025]);
}

#[test]
fn previous_and_unsupported_guests_reject_network_updates_without_changing_disks() {
  let (_root, manager, mut machine) = fixture("ubuntu");
  let previous = manager.root.join("lima").join(&machine.id);
  std::fs::create_dir_all(&previous).unwrap();
  let disk = previous.join("diffdisk");
  std::fs::write(&disk, b"preserved").unwrap();
  assert!(network::set_connected(&manager, &machine.id, false).is_err());
  assert!(!manager.root.join("network").exists());
  assert_eq!(std::fs::read(&disk).unwrap(), b"preserved");
  std::fs::remove_dir_all(previous).unwrap();
  for runtime in [None, Some(model::MachineRuntime::Hypervisor)] {
    machine.runtime = runtime;
    machine.guest = if runtime.is_some() {
      model::GuestOs::Windows
    } else {
      model::GuestOs::Linux
    };
    store::json::write(
      &manager
        .root
        .join("records")
        .join(format!("{}.json", machine.id)),
      &machine,
    )
    .unwrap();
    assert!(network::connected(&manager, &machine.id, Actor::Person).is_err());
    assert!(network::set_connected(&manager, &machine.id, false).is_err());
    assert!(!manager.root.join("network").exists());
  }
}
