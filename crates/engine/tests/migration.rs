#![cfg(unix)]

use engine::machines::{
  native::{config, sessions::Sessions},
  Actor, Machines,
};
use model::{EngineResources, GuestOs, Machine, MachineInput};

fn fixture() -> (tempfile::TempDir, Machines, Machine) {
  let root = tempfile::tempdir().unwrap();
  let manager = Machines {
    root: root.path().into(),
  };
  let machine = Machine {
    id: model::new_uuid(),
    name: "Previous Windows".into(),
    guest: GuestOs::Windows,
    profile: "windows".into(),
    resources: EngineResources {
      cpus: 2,
      memory_gib: 4,
      disk_gib: 64,
    },
    agent_access: true,
    agent_generation: 0,
    runtime: None,
    installer: None,
  };
  store::json::write(
    &manager
      .root
      .join("records")
      .join(format!("{}.json", machine.id)),
    &machine,
  )
  .unwrap();
  (root, manager, machine)
}

#[tokio::test]
async fn previous_windows_operations_reject_before_helpers_or_guest_file_changes() {
  let (root, manager, machine) = fixture();
  let previous = manager.root.join("lima").join(&machine.id);
  std::fs::create_dir_all(&previous).unwrap();
  let disk = previous.join("diffdisk");
  std::fs::write(&disk, b"preserved previous disk").unwrap();
  let record = manager
    .root
    .join("records")
    .join(format!("{}.json", machine.id));
  let original = std::fs::read(&record).unwrap();
  let id = &machine.id;
  for result in [
    manager.start(id, Actor::Person).await,
    manager.stop(id, Actor::Agent).await,
    manager
      .exec(id, Actor::Agent, &["whoami".into()])
      .await
      .map(|_| ()),
    manager
      .input(
        id,
        Actor::Agent,
        MachineInput::Key {
          keys: "ctrl+alt+delete".into(),
        },
      )
      .await,
    manager
      .screenshot(id, Actor::Agent, &root.path().join("screen.png"))
      .await,
    manager
      .copy(
        id,
        Actor::Agent,
        &root.path().join("source"),
        "/guest",
        true,
      )
      .await,
  ] {
    assert!(result
      .unwrap_err()
      .to_string()
      .contains("Migration required"));
  }
  assert_eq!(std::fs::read(disk).unwrap(), b"preserved previous disk");
  assert_eq!(std::fs::read(record).unwrap(), original);
  assert!(!root.path().join("screen.png").exists());
  assert!(!root.path().join("progress").exists());
  assert!(!root.path().join("native").exists());
  assert!(!root.path().join("configs").exists());
}

#[tokio::test]
async fn untagged_windows_records_list_as_migration_without_old_instance_or_helper() {
  let (_root, manager, machine) = fixture();
  let rows = manager.list(Actor::Person).await.unwrap();
  assert_eq!(rows.len(), 1);
  assert_eq!(rows[0].state, "Migration required");
  let rows = Sessions::new(manager.clone())
    .list_windows(Actor::Agent)
    .await
    .unwrap();
  assert_eq!(rows[0].state, "Migration required");
  assert!(config::initialize(&manager, &machine.id).is_err());
  assert!(!manager.root.join("native").exists());
  manager.set_agent_access(&machine.id, false).unwrap();
  assert!(manager.list(Actor::Agent).await.unwrap().is_empty());
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[tokio::test]
async fn manager_creation_uses_native_windows_records_without_template_files() {
  let (_root, manager, _) = fixture();
  let machine = manager
    .create(model::CreateMachine {
      name: "Owned Windows".into(),
      profile: "windows".into(),
      resources: EngineResources {
        cpus: 2,
        memory_gib: 4,
        disk_gib: 64,
      },
      agent_access: true,
      installer: None,
    })
    .await
    .unwrap();
  assert_eq!(machine.runtime, Some(model::MachineRuntime::Hypervisor));
  assert!(!manager.root.join("lima").exists());
  assert!(!manager.root.join("configs").exists());
}
