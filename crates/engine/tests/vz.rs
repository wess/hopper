#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use engine::machines::{
  vz::{self, Action, Service},
  Actor, Machines,
};
use fs2::FileExt;
use model::{GuestOs, Machine};
use std::{fs::OpenOptions, sync::Arc};

const ID: &str = "00000000-0000-0000-0000-000000000001";

fn fixture() -> (tempfile::TempDir, Machines) {
  let root = tempfile::tempdir().unwrap();
  let manager = Machines {
    root: root.path().join("machines"),
  };
  let machine = Machine {
    id: ID.into(),
    name: "Native Linux".into(),
    guest: GuestOs::Linux,
    profile: "linux".into(),
    resources: model::EngineResources {
      cpus: 2,
      memory_gib: 4,
      disk_gib: 64,
    },
    agent_access: true,
    agent_generation: 0,
    installer: None,
  };
  store::json::write(
    &manager.root.join("records").join(format!("{ID}.json")),
    &machine,
  )
  .unwrap();
  (root, manager)
}

#[tokio::test]
async fn queued_revocation_and_off_on_changes_reject_original_policy() {
  for reenable in [false, true] {
    let (_root, manager) = fixture();
    let (client, mut owner) = vz::channel();
    let wake = owner.wake();
    let service = Service::new(manager.clone(), client);
    let request =
      tokio::spawn(async move { service.transition(ID, Actor::Agent, Action::Start).await });
    wake.notified().await;
    manager.set_agent_access(ID, false).unwrap();
    if reenable {
      manager.set_agent_access(ID, true).unwrap();
    }
    owner.tick();
    let error = request.await.unwrap().unwrap_err().to_string();
    assert!(
      error.contains("Agent access") || error.contains("agent policy"),
      "{error}"
    );
    assert!(!owner.active());
    assert!(!manager.root.join("native").exists());
    assert!(!manager.root.join("lima").exists());
  }
}

#[tokio::test]
async fn cancelled_queue_keeps_operation_lease_until_owner_skips_dispatch() {
  let (_root, manager) = fixture();
  let (client, mut owner) = vz::channel();
  let wake = owner.wake();
  let service = Service::new(manager.clone(), client);
  let request =
    tokio::spawn(async move { service.transition(ID, Actor::Person, Action::Start).await });
  wake.notified().await;
  let lock = OpenOptions::new()
    .read(true)
    .write(true)
    .open(manager.root.join("locks").join(ID))
    .unwrap();
  assert!(lock.try_lock_exclusive().is_err());
  request.abort();
  assert!(request.await.unwrap_err().is_cancelled());
  assert!(lock.try_lock_exclusive().is_err());
  owner.tick();
  lock.try_lock_exclusive().unwrap();
  FileExt::unlock(&lock).unwrap();
  assert!(!owner.active());
}

#[tokio::test]
async fn platform_and_legacy_rejections_preserve_prior_disks() {
  let (_root, manager) = fixture();
  let (client, _owner) = vz::channel();
  let service = Arc::new(Service::new(manager.clone(), client));
  let legacy = manager.root.join("lima").join(ID);
  std::fs::create_dir_all(&legacy).unwrap();
  let disk = legacy.join("diffdisk");
  std::fs::write(&disk, b"previous disk").unwrap();
  let error = service
    .transition(ID, Actor::Person, Action::Start)
    .await
    .unwrap_err();
  assert!(error.to_string().contains("requires migration"));
  assert_eq!(std::fs::read(&disk).unwrap(), b"previous disk");
  let mut machine = manager.machine(ID, Actor::Person).unwrap();
  machine.guest = GuestOs::Windows;
  store::json::write(
    &manager.root.join("records").join(format!("{ID}.json")),
    &machine,
  )
  .unwrap();
  let error = service
    .transition(ID, Actor::Person, Action::Start)
    .await
    .unwrap_err();
  assert!(error.to_string().contains("Hypervisor"));
  assert_eq!(std::fs::read(&disk).unwrap(), b"previous disk");
  assert!(!manager.root.join("locks").join(ID).exists());
}
