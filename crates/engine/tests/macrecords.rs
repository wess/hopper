#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use engine::machines::{vz::records, Actor, Machines};
use model::{CreateMachine, EngineResources, GuestOs, MachineRuntime};
use std::os::unix::fs::PermissionsExt;

fn fixture() -> (tempfile::TempDir, Machines, CreateMachine) {
  let root = tempfile::tempdir().unwrap();
  let manager = Machines {
    root: root.path().join("machines"),
  };
  let request = CreateMachine {
    name: "  Native macOS  ".into(),
    profile: "macos".into(),
    resources: EngineResources {
      cpus: 2,
      memory_gib: 4,
      disk_gib: 64,
    },
    agent_access: true,
    installer: None,
  };
  (root, manager, request)
}

#[tokio::test]
async fn mac_creation_selects_native_runtime_and_reports_pending_installation() {
  let (_root, manager, request) = fixture();
  let machine = manager.create(request).await.unwrap();
  assert_eq!(machine.guest, GuestOs::Macos);
  assert_eq!(machine.runtime, Some(MachineRuntime::Virtualization));
  assert_eq!(machine.name, "Native macOS");
  assert!(
    manager
      .machine(&machine.id, Actor::Agent)
      .unwrap()
      .agent_access
  );
  let rows = manager.list_non_windows(Actor::Person).await.unwrap();
  assert_eq!(rows.len(), 1);
  assert_eq!(rows[0].state, "Setup required");
  assert!(!rows[0].busy);
  assert!(rows[0]
    .progress
    .as_deref()
    .unwrap()
    .contains("in development"));
  for path in ["lima", "configs", "vz", "native"] {
    assert!(!manager.root.join(path).exists());
  }
  assert!(manager.start(&machine.id, Actor::Person).await.is_err());
  assert!(manager.stop(&machine.id, Actor::Person).await.is_err());
  manager.set_agent_access(&machine.id, false).unwrap();
  assert!(manager
    .list_non_windows(Actor::Agent)
    .await
    .unwrap()
    .is_empty());
  manager.set_agent_access(&machine.id, true).unwrap();
  assert_eq!(
    manager
      .machine(&machine.id, Actor::Agent)
      .unwrap()
      .agent_generation,
    2
  );
}

#[tokio::test]
async fn existing_mac_guest_data_is_preserved_and_requires_migration() {
  let (_root, manager, request) = fixture();
  let machine = records::create(&manager, request).unwrap();
  let previous = manager.root.join("lima").join(&machine.id);
  std::fs::create_dir_all(&previous).unwrap();
  std::fs::write(previous.join("disk"), b"previous mac guest").unwrap();
  let rows = manager.list_non_windows(Actor::Person).await.unwrap();
  assert_eq!(rows[0].state, "Migration required");
  assert!(manager.start(&machine.id, Actor::Person).await.is_err());
  assert_eq!(
    std::fs::read(previous.join("disk")).unwrap(),
    b"previous mac guest"
  );
}

#[test]
fn invalid_platform_resources_and_restore_paths_do_not_publish_records() {
  let (root, manager, request) = fixture();
  for profile in ["windows", "fedora", ""] {
    let mut invalid = request.clone();
    invalid.profile = profile.into();
    assert!(records::create(&manager, invalid).is_err());
  }
  for (cpus, memory_gib, disk_gib) in [(1, 4, 64), (2, 3, 64), (2, 4, 63)] {
    let mut invalid = request.clone();
    invalid.resources = EngineResources {
      cpus,
      memory_gib,
      disk_gib,
    };
    assert!(records::create(&manager, invalid).is_err());
  }
  let image = root.path().join("restore.ipsw");
  std::fs::write(&image, b"synthetic restore media").unwrap();
  let alias = root.path().join("alias.ipsw");
  std::os::unix::fs::symlink(&image, &alias).unwrap();
  for path in [
    "relative.ipsw".into(),
    alias,
    root.path().join("missing.ipsw"),
    root.path().into(),
  ] {
    let mut invalid = request.clone();
    invalid.installer = Some(path.to_str().unwrap().into());
    assert!(records::create(&manager, invalid).is_err());
  }
  assert!(!manager.root.join("records").exists());
  let mut valid = request;
  valid.installer = Some(image.to_str().unwrap().into());
  let machine = records::create(&manager, valid).unwrap();
  assert_eq!(machine.installer.as_deref(), image.to_str());
  assert_eq!(std::fs::read(&image).unwrap(), b"synthetic restore media");
}

#[tokio::test]
async fn mac_listing_respects_operation_and_runtime_ownership_and_ignores_linux_journals() {
  let (_root, manager, request) = fixture();
  let machine = records::create(&manager, request).unwrap();
  let operation = std::fs::OpenOptions::new()
    .read(true)
    .write(true)
    .open(manager.root.join("locks").join(&machine.id))
    .unwrap();
  let held = store::lock::exclusive(operation).unwrap();
  let rows = manager.list_non_windows(Actor::Person).await.unwrap();
  assert!(rows[0].busy);
  assert!(!manager
    .root
    .join("locks")
    .join(format!("{}.runtime", machine.id))
    .exists());
  drop(held);
  manager.list_non_windows(Actor::Person).await.unwrap();
  let runtime = std::fs::OpenOptions::new()
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
  assert!(rows[0].busy);
  assert!(rows[0].progress.as_deref().unwrap().contains("owned"));
  drop(held);
  let target = manager.root.join("vz").join(&machine.id);
  std::fs::create_dir_all(&target).unwrap();
  std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o700)).unwrap();
  std::fs::write(target.join("installation"), b"invalid Linux-only journal").unwrap();
  let rows = manager.list_non_windows(Actor::Person).await.unwrap();
  assert_eq!(rows[0].state, "Setup required");
  assert_eq!(
    std::fs::read(target.join("installation")).unwrap(),
    b"invalid Linux-only journal"
  );
}

#[tokio::test]
async fn symlinked_mac_state_is_rejected_without_changing_its_target() {
  let (root, manager, request) = fixture();
  let machine = records::create(&manager, request).unwrap();
  let sentinel = root.path().join("sentinel");
  std::fs::create_dir(&sentinel).unwrap();
  std::fs::write(sentinel.join("data"), b"preserved").unwrap();
  std::fs::create_dir_all(manager.root.join("vz")).unwrap();
  std::os::unix::fs::symlink(&sentinel, manager.root.join("vz").join(&machine.id)).unwrap();
  assert!(manager.list_non_windows(Actor::Person).await.is_err());
  assert_eq!(std::fs::read(sentinel.join("data")).unwrap(), b"preserved");
}
