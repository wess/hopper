#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use engine::machines::{
  vz::{mac::platform, records},
  Machines,
};
use machine::vz::{queue::Check, restore::Image};
use std::{
  os::unix::fs::{MetadataExt, PermissionsExt},
  sync::Arc,
};

fn fixture() -> (tempfile::TempDir, Machines, model::Machine, Image) {
  let root = tempfile::tempdir().unwrap();
  let manager = Machines {
    root: root.path().join("machines"),
  };
  let machine = records::create(
    &manager,
    model::CreateMachine {
      name: "Platform fixture".into(),
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
  let image = Image {
    url: "https://updates.cdn-apple.com/fixture.ipsw".into(),
    build: "fixture".into(),
    version: [26, 0, 0],
    hardware: vec![1],
    minimum_cpus: 2,
    minimum_memory: 4 << 30,
  };
  (root, manager, machine, image)
}

#[test]
fn mac_preparation_keeps_sparse_private_state_unpublished_until_native_admission() {
  let (_root, manager, machine, image) = fixture();
  let check: Check = Arc::new(|| Ok(()));
  let prepared = platform::prepare(&manager, machine.clone(), image, check).unwrap();
  let directory = prepared.directory().to_owned();
  let disk = std::fs::metadata(directory.join("disk")).unwrap();
  assert_eq!(disk.len(), 64 << 30);
  assert_eq!(disk.blocks(), 0);
  assert_eq!(disk.permissions().mode() & 0o777, 0o600);
  assert_eq!(
    std::fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
    0o700
  );
  assert!(!manager.root.join("vz").join(&machine.id).exists());
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
  assert!(store::lock::exclusive(runtime.try_clone().unwrap()).is_err());
  drop(prepared);
  assert!(!directory.exists());
  assert_eq!(
    std::fs::read_dir(manager.root.join("vz")).unwrap().count(),
    0
  );
  drop(store::lock::exclusive(runtime).unwrap());
}

#[test]
fn invalid_requirements_and_policy_cancellation_do_not_publish_mac_state() {
  let (_root, manager, machine, image) = fixture();
  for field in [0, 1, 2, 3] {
    let mut bad = image.clone();
    match field {
      0 => bad.minimum_cpus = 3,
      1 => bad.minimum_memory = (4 << 30) + 1,
      2 => bad.hardware.clear(),
      _ => bad.build.clear(),
    }
    assert!(platform::prepare(&manager, machine.clone(), bad, Arc::new(|| Ok(()))).is_err());
  }
  assert!(!manager.root.join("vz").exists());
  let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
  let check: Check = Arc::new(move || {
    anyhow::ensure!(
      calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0,
      "revoked"
    );
    Ok(())
  });
  assert!(platform::prepare(&manager, machine, image, check).is_err());
  assert_eq!(
    std::fs::read_dir(manager.root.join("vz")).unwrap().count(),
    0
  );
}

#[test]
fn saved_platform_reuse_preserves_data_and_rejects_foreign_identity_and_hardware() {
  let (_root, manager, machine, image) = fixture();
  let target = manager.root.join("vz").join(&machine.id);
  std::fs::create_dir_all(target.join("auxiliary")).unwrap();
  for path in [&manager.root.join("vz"), &target, &target.join("auxiliary")] {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
  }
  let saved = platform::Platform {
    id: machine.id.clone(),
    version: 1,
    disk_gib: 64,
    identity: vec![1],
    hardware: image.hardware.clone(),
    build: image.build.clone(),
    os_version: image.version,
    minimum_cpus: image.minimum_cpus,
    minimum_memory: image.minimum_memory,
  };
  let bytes = serde_json::to_vec(&saved).unwrap();
  for (name, bytes) in [
    ("platform", bytes.as_slice()),
    ("auxiliary/hardware", &[1][..]),
    ("auxiliary/state", b"owned state".as_slice()),
  ] {
    std::fs::write(target.join(name), bytes).unwrap();
    std::fs::set_permissions(target.join(name), std::fs::Permissions::from_mode(0o600)).unwrap();
  }
  let disk = std::fs::File::create(target.join("disk")).unwrap();
  disk.set_len(64 << 30).unwrap();
  std::fs::set_permissions(target.join("disk"), std::fs::Permissions::from_mode(0o600)).unwrap();
  let prepared = platform::prepare(
    &manager,
    machine.clone(),
    image.clone(),
    Arc::new(|| Ok(())),
  )
  .unwrap();
  assert_eq!(prepared.directory(), target);
  drop(prepared);
  let mut foreign = image.clone();
  foreign.hardware = vec![2];
  assert!(platform::prepare(&manager, machine.clone(), foreign, Arc::new(|| Ok(()))).is_err());
  let mut resized = machine.clone();
  resized.resources.disk_gib = 65;
  assert!(platform::prepare(&manager, resized, image.clone(), Arc::new(|| Ok(()))).is_err());
  let mut foreign: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
  foreign["id"] = serde_json::json!(model::new_uuid());
  let foreign = serde_json::to_vec(&foreign).unwrap();
  std::fs::write(target.join("platform"), &foreign).unwrap();
  assert!(platform::prepare(
    &manager,
    machine.clone(),
    image.clone(),
    Arc::new(|| Ok(()))
  )
  .is_err());
  assert_eq!(std::fs::read(target.join("platform")).unwrap(), foreign);
  std::fs::write(target.join("platform"), &bytes).unwrap();
  assert_eq!(std::fs::read(target.join("platform")).unwrap(), bytes);
  assert_eq!(
    std::fs::read(target.join("auxiliary/state")).unwrap(),
    b"owned state"
  );
}
