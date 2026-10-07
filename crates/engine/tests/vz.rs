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

fn installer(manager: &Machines) -> std::path::PathBuf {
  use std::io::{Seek, SeekFrom, Write};
  let path = manager.root.join("installer.iso");
  let mut file = std::fs::File::create(&path).unwrap();
  file.set_len(65536).unwrap();
  file.seek(SeekFrom::Start(32768)).unwrap();
  file.write_all(b"\x01CD001\x01").unwrap();
  let mut machine = manager.machine(ID, Actor::Person).unwrap();
  machine.installer = Some(path.to_str().unwrap().into());
  store::json::write(
    &manager.root.join("records").join(format!("{ID}.json")),
    &machine,
  )
  .unwrap();
  path
}

#[tokio::test]
async fn linux_preparation_is_sparse_private_and_cleans_unpublished_state() {
  use std::os::unix::fs::{MetadataExt, PermissionsExt};
  let (_root, manager) = fixture();
  let media = installer(&manager);
  let original = std::fs::read(&media).unwrap();
  let (client, _owner) = vz::channel();
  let service = Service::new(manager.clone(), client);
  let prepared = service
    .prepare_linux(ID, Actor::Agent, vz::Stage::Installer)
    .await
    .unwrap();
  let parent = manager.root.join("vz");
  assert!(!parent.join(ID).exists());
  let stage = std::fs::read_dir(&parent)
    .unwrap()
    .next()
    .unwrap()
    .unwrap()
    .path();
  let info = std::fs::metadata(stage.join("disk")).unwrap();
  assert_eq!(info.len(), 64 << 30);
  assert_eq!(info.blocks(), 0);
  assert_eq!(info.permissions().mode() & 0o777, 0o600);
  assert_eq!(
    std::fs::metadata(&stage).unwrap().permissions().mode() & 0o777,
    0o700
  );
  drop(prepared);
  assert!(!stage.exists());
  assert_eq!(std::fs::read(media).unwrap(), original);
  let lock = OpenOptions::new()
    .read(true)
    .write(true)
    .open(manager.root.join("locks").join(format!("{ID}.runtime")))
    .unwrap();
  lock.try_lock_exclusive().unwrap();
  FileExt::unlock(&lock).unwrap();
  assert!(!manager.root.join("lima").exists());
}

#[tokio::test]
async fn linux_preparation_preserves_bad_existing_state_and_symlink_targets() {
  use std::os::unix::fs::PermissionsExt;
  let (_root, manager) = fixture();
  installer(&manager);
  let parent = manager.root.join("vz");
  std::fs::create_dir(&parent).unwrap();
  std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o700)).unwrap();
  let target = parent.join(ID);
  std::fs::create_dir(&target).unwrap();
  std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o700)).unwrap();
  let sentinel = target.join("sentinel");
  std::fs::write(&sentinel, b"retained state").unwrap();
  let (client, _owner) = vz::channel();
  let service = Service::new(manager.clone(), client);
  assert!(service
    .prepare_linux(ID, Actor::Person, vz::Stage::Installer)
    .await
    .is_err());
  assert_eq!(std::fs::read(&sentinel).unwrap(), b"retained state");
  let previous = parent.join("previous");
  std::fs::rename(&target, &previous).unwrap();
  std::os::unix::fs::symlink(&previous, &target).unwrap();
  assert!(service
    .prepare_linux(ID, Actor::Person, vz::Stage::Installer)
    .await
    .is_err());
  assert_eq!(
    std::fs::read(previous.join("sentinel")).unwrap(),
    b"retained state"
  );
  assert!(std::fs::symlink_metadata(target)
    .unwrap()
    .file_type()
    .is_symlink());
}

#[tokio::test]
async fn linux_preparation_requires_install_media_and_refuses_unprepared_system_boot() {
  let (_root, manager) = fixture();
  let (client, _owner) = vz::channel();
  let service = Service::new(manager.clone(), client);
  assert!(service
    .prepare_linux(ID, Actor::Person, vz::Stage::Installer)
    .await
    .is_err());
  assert!(!manager.root.join("vz").exists());
  assert!(service
    .prepare_linux(ID, Actor::Person, vz::Stage::System)
    .await
    .is_err());
  assert!(!manager.root.join("vz").join(ID).exists());
  let media = installer(&manager);
  std::fs::write(&media, b"not installation media").unwrap();
  assert!(service
    .prepare_linux(ID, Actor::Person, vz::Stage::Installer)
    .await
    .is_err());
  assert_eq!(std::fs::read(media).unwrap(), b"not installation media");
}

#[tokio::test]
async fn native_state_never_falls_back_to_previous_runtime_operations() {
  let (_root, manager) = fixture();
  let target = manager.root.join("vz").join(ID);
  std::fs::create_dir_all(&target).unwrap();
  let sentinel = target.join("disk");
  std::fs::write(&sentinel, b"preserved native data").unwrap();
  let rows = manager.list_non_windows(Actor::Person).await.unwrap();
  assert_eq!(rows.len(), 1);
  assert_eq!(rows[0].state, "Unavailable");
  assert!(rows[0].busy);
  assert!(manager.start(ID, Actor::Person).await.is_err());
  assert!(manager.stop(ID, Actor::Person).await.is_err());
  assert!(manager
    .exec(ID, Actor::Agent, &["true".into()])
    .await
    .is_err());
  assert!(manager.viewer_pid(ID).await.is_err());
  assert!(manager
    .clone_machine(ID, Actor::Agent, "clone")
    .await
    .is_err());
  assert_eq!(std::fs::read(&sentinel).unwrap(), b"preserved native data");
  assert!(!manager.root.join("lima").exists());
}
