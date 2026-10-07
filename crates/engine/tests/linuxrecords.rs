#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use engine::machines::{linux::records, Actor, Machines};
use model::{CreateMachine, EngineResources, MachineRuntime};
use std::{fs::OpenOptions, os::unix::fs::PermissionsExt};

fn fixture() -> (tempfile::TempDir, Machines, CreateMachine) {
  let root = tempfile::tempdir().unwrap();
  let manager = Machines {
    root: root.path().join("machines"),
  };
  let request = CreateMachine {
    name: "  Native Ubuntu  ".into(),
    profile: "ubuntu".into(),
    resources: EngineResources::default(),
    installer: None,
    agent_access: true,
  };
  (root, manager, request)
}

#[tokio::test]
async fn native_creation_persists_runtime_and_policy_without_previous_helpers_or_disks() {
  let (_root, manager, request) = fixture();
  let machine = records::create(&manager, request.clone()).unwrap();
  assert_eq!(machine.name, "Native Ubuntu");
  assert_eq!(machine.runtime, Some(MachineRuntime::Virtualization));
  assert!(
    manager
      .machine(&machine.id, Actor::Agent)
      .unwrap()
      .agent_access
  );
  let rows = manager.list_non_windows(Actor::Person).await.unwrap();
  assert_eq!(rows.len(), 1);
  assert_eq!(rows[0].state, "Not created");
  assert!(!rows[0].busy);
  for path in ["lima", "configs", "vz", "native"] {
    assert!(!manager.root.join(path).exists());
  }
  assert!(manager.start(&machine.id, Actor::Person).await.is_err());
  let mut disabled = request;
  disabled.agent_access = false;
  let copy = records::create(&manager, disabled).unwrap();
  assert_ne!(machine.id, copy.id);
  assert!(manager.machine(&copy.id, Actor::Agent).is_err());
  manager.set_agent_access(&machine.id, false).unwrap();
  manager.set_agent_access(&machine.id, true).unwrap();
  let updated = manager.machine(&machine.id, Actor::Agent).unwrap();
  assert_eq!(updated.runtime, Some(MachineRuntime::Virtualization));
  assert_eq!(updated.agent_generation, 2);
}

#[tokio::test]
async fn listing_distinguishes_ready_preparation_and_foreign_ownership_without_probing_during_operations(
) {
  let (_root, manager, request) = fixture();
  let machine = records::create(&manager, request).unwrap();
  let target = manager.root.join("vz").join(&machine.id);
  std::fs::create_dir_all(&target).unwrap();
  std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o700)).unwrap();
  let operation = OpenOptions::new()
    .read(true)
    .write(true)
    .open(manager.root.join("locks").join(&machine.id))
    .unwrap();
  let held = store::lock::exclusive(operation).unwrap();
  let rows = manager.list_non_windows(Actor::Person).await.unwrap();
  assert!(rows[0].busy);
  assert!(rows[0].progress.as_deref().unwrap().contains("operation"));
  assert!(!manager
    .root
    .join("locks")
    .join(format!("{}.runtime", machine.id))
    .exists());
  drop(held);
  let rows = manager.list_non_windows(Actor::Person).await.unwrap();
  assert_eq!(rows[0].state, "Ready to start");
  assert!(!rows[0].busy);
  let runtime = OpenOptions::new()
    .read(true)
    .write(true)
    .open(
      manager
        .root
        .join("locks")
        .join(format!("{}.runtime", machine.id)),
    )
    .unwrap();
  let held = store::lock::exclusive(runtime).unwrap();
  let rows = manager.list_non_windows(Actor::Person).await.unwrap();
  assert_eq!(rows[0].state, "Unavailable");
  assert!(rows[0].busy);
  drop(held);
}

#[tokio::test]
async fn explicit_runtime_does_not_override_previous_guest_data_or_invalid_platforms() {
  let (_root, manager, request) = fixture();
  let mut machine = records::create(&manager, request).unwrap();
  let old = manager.root.join("lima").join(&machine.id);
  std::fs::create_dir_all(&old).unwrap();
  let disk = old.join("disk");
  std::fs::write(&disk, b"previous guest data").unwrap();
  let rows = manager.list_non_windows(Actor::Person).await.unwrap();
  assert_eq!(rows[0].state, "Migration required");
  assert_eq!(std::fs::read(&disk).unwrap(), b"previous guest data");
  machine.runtime = Some(MachineRuntime::Hypervisor);
  store::json::write(
    &manager
      .root
      .join("records")
      .join(format!("{}.json", machine.id)),
    &machine,
  )
  .unwrap();
  assert!(manager.machine(&machine.id, Actor::Person).is_err());
  assert!(manager.list_non_windows(Actor::Person).await.is_err());
  assert_eq!(std::fs::read(disk).unwrap(), b"previous guest data");
}

#[tokio::test]
async fn listing_rejects_readable_native_state_without_changing_its_files() {
  let (_root, manager, request) = fixture();
  let machine = records::create(&manager, request).unwrap();
  let target = manager.root.join("vz").join(&machine.id);
  std::fs::create_dir_all(&target).unwrap();
  std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755)).unwrap();
  let preserved = target.join("disk");
  std::fs::write(&preserved, b"preserved VM data").unwrap();
  assert!(manager
    .list_non_windows(Actor::Person)
    .await
    .unwrap_err()
    .to_string()
    .contains("private and owned"));
  assert_eq!(std::fs::read(&preserved).unwrap(), b"preserved VM data");
  std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o700)).unwrap();
  assert_eq!(
    manager.list_non_windows(Actor::Person).await.unwrap()[0].state,
    "Ready to start"
  );
}
