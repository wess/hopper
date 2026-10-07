#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use engine::machines::{
  linux::records,
  vz::{self, Action, Service, Stage},
  Actor, Machines,
};
use std::os::unix::fs::PermissionsExt;

#[path = "support/linux.rs"]
mod fixture;

fn machine() -> (tempfile::TempDir, Machines, String) {
  let root = tempfile::tempdir().unwrap();
  let media = fixture::media(root.path());
  let manager = Machines {
    root: root.path().join("machines"),
  };
  let record = records::create(
    &manager,
    model::CreateMachine {
      name: "Stop fixture".into(),
      profile: "ubuntu".into(),
      resources: model::EngineResources {
        cpus: 2,
        memory_gib: 4,
        disk_gib: 32,
      },
      agent_access: true,
      installer: Some(media.to_str().unwrap().into()),
    },
  )
  .unwrap();
  (root, manager, record.id)
}

#[tokio::test]
async fn stop_cancels_preparation_even_while_its_operation_lock_is_held() {
  let (_root, manager, id) = machine();
  let (client, _) = vz::channel();
  let service = Service::new(manager.clone(), client);
  let prepared = service
    .prepare_linux(&id, Actor::Agent, Stage::Installer)
    .await
    .unwrap();
  assert!(service
    .transition(&id, Actor::Person, Action::Stop)
    .await
    .is_err());
  let plan = engine::machines::linux::provision::Plan {
    user_data: "#cloud-config\n".into(),
    meta_data: "owned".into(),
  };
  let error = prepared.provision(&plan).err().unwrap().to_string();
  assert!(error.contains("cancelled by an explicit stop"), "{error}");
  assert_eq!(
    std::fs::read_dir(manager.root.join("vz")).unwrap().count(),
    0
  );
  let intent = manager.root.join("intent").join(id);
  assert_eq!(
    std::fs::metadata(intent).unwrap().permissions().mode() & 0o777,
    0o600
  );
}

#[tokio::test]
async fn queued_start_is_cancelled_by_a_later_stop_before_dispatch() {
  let (_root, manager, id) = machine();
  let (client, mut owner) = vz::channel();
  let wake = owner.wake();
  let service = Service::new(manager.clone(), client);
  let control = service.clone();
  let identity = id.clone();
  let start = tokio::spawn(async move {
    control
      .transition(&identity, Actor::Agent, Action::Start)
      .await
  });
  wake.notified().await;
  assert!(service
    .transition(&id, Actor::Person, Action::Stop)
    .await
    .is_err());
  owner.tick();
  let error = start.await.unwrap().unwrap_err().to_string();
  assert!(error.contains("cancelled by an explicit stop"), "{error}");
  assert!(!owner.active());
}

#[tokio::test]
async fn revoked_stop_cannot_write_an_intent_or_refresh_a_queued_watch_policy() {
  let (_root, manager, id) = machine();
  manager.set_agent_access(&id, false).unwrap();
  let (client, mut owner) = vz::channel();
  let wake = owner.wake();
  let service = Service::new(manager.clone(), client);
  assert!(service
    .transition(&id, Actor::Agent, Action::Stop)
    .await
    .is_err());
  assert!(!manager.root.join("intent").exists());
  manager.set_agent_access(&id, true).unwrap();
  let identity = id.clone();
  let pending = tokio::spawn(async move {
    service
      .watch_installation(
        &identity,
        Actor::Agent,
        "8197e0f0-0603-43e9-a817-eaf7ab0327af",
      )
      .await
  });
  wake.notified().await;
  manager.set_agent_access(&id, false).unwrap();
  manager.set_agent_access(&id, true).unwrap();
  owner.tick();
  let error = pending.await.unwrap().err().unwrap().to_string();
  assert!(error.contains("agent policy"), "{error}");
  assert!(!manager.root.join("intent").exists());
}

#[tokio::test]
async fn startup_rejects_symlinked_stop_intents_without_reading_or_changing_the_target() {
  let (root, manager, id) = machine();
  let sentinel = root.path().join("sentinel");
  std::fs::write(&sentinel, b"preserved data").unwrap();
  let directory = manager.root.join("intent");
  std::fs::create_dir(&directory).unwrap();
  std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
  std::os::unix::fs::symlink(&sentinel, directory.join(&id)).unwrap();
  let (client, _) = vz::channel();
  let service = Service::new(manager.clone(), client);
  assert!(service
    .prepare_linux(&id, Actor::Agent, Stage::Installer)
    .await
    .is_err());
  assert_eq!(std::fs::read(sentinel).unwrap(), b"preserved data");
  assert!(!manager.root.join("vz").exists());
}
