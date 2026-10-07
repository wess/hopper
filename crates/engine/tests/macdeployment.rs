#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use engine::machines::{
  vz::{
    mac::{deployment, platform},
    records,
  },
  Actor, Machines,
};
use machine::vz::restore::Image;
use std::{os::unix::fs::PermissionsExt, sync::Arc};

fn fixture() -> (
  tempfile::TempDir,
  Machines,
  model::Machine,
  Image,
  std::path::PathBuf,
) {
  let root = tempfile::tempdir().unwrap();
  let manager = Machines {
    root: root.path().join("machines"),
  };
  let machine = records::create(
    &manager,
    model::CreateMachine {
      name: "Deployment fixture".into(),
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
    url: String::new(),
    build: "fixture".into(),
    version: [26, 0, 0],
    hardware: vec![1],
    minimum_cpus: 2,
    minimum_memory: 4 << 30,
  };
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
    minimum_cpus: 2,
    minimum_memory: 4 << 30,
  };
  for (name, bytes) in [
    ("platform", serde_json::to_vec(&saved).unwrap()),
    ("auxiliary/hardware", vec![1]),
    ("auxiliary/state", b"owned state".to_vec()),
  ] {
    std::fs::write(target.join(name), bytes).unwrap();
    std::fs::set_permissions(target.join(name), std::fs::Permissions::from_mode(0o600)).unwrap();
  }
  let disk = std::fs::File::create(target.join("disk")).unwrap();
  disk.set_len(64 << 30).unwrap();
  std::fs::set_permissions(target.join("disk"), std::fs::Permissions::from_mode(0o600)).unwrap();
  (root, manager, machine, image, target)
}

#[tokio::test]
async fn deployment_receipt_is_private_bound_and_monotonic_without_claiming_sdk_installation() {
  let (_root, manager, machine, _image, target) = fixture();
  let attempt = deployment::begin(&target, &machine.id).unwrap();
  assert_eq!(
    deployment::read(&target, &machine.id).unwrap(),
    Some(deployment::Phase::Installing)
  );
  assert_eq!(
    std::fs::metadata(target.join("deployment"))
      .unwrap()
      .permissions()
      .mode()
      & 0o777,
    0o600
  );
  assert!(deployment::begin(&target, &machine.id).is_err());
  assert!(deployment::read(&target, &model::new_uuid()).is_err());
  deployment::installed(&attempt).unwrap();
  assert_eq!(
    deployment::read(&target, &machine.id).unwrap(),
    Some(deployment::Phase::Installed)
  );
  let bytes = std::fs::read(target.join("deployment")).unwrap();
  assert!(deployment::installed(&attempt).is_err());
  assert_eq!(std::fs::read(target.join("deployment")).unwrap(), bytes);
  let prepared = platform::system(&manager, machine, Arc::new(|| Ok(()))).unwrap();
  assert_eq!(prepared.directory(), target);
  drop(prepared);
  let rows = manager.list_non_windows(Actor::Person).await.unwrap();
  assert_eq!(rows[0].state, "Ready to start");
  assert!(rows[0].progress.as_deref().unwrap().contains("unverified"));
}

#[tokio::test]
async fn interrupted_installation_rejects_reinstallation_and_preserves_written_data() {
  let (_root, manager, machine, image, target) = fixture();
  drop(deployment::begin(&target, &machine.id).unwrap());
  use std::io::Write;
  std::fs::OpenOptions::new()
    .write(true)
    .open(target.join("disk"))
    .unwrap()
    .write_all(b"guest data")
    .unwrap();
  let before = std::fs::read(target.join("deployment")).unwrap();
  assert!(platform::prepare(&manager, machine.clone(), image, Arc::new(|| Ok(()))).is_err());
  assert!(platform::system(&manager, machine.clone(), Arc::new(|| Ok(()))).is_err());
  assert!(deployment::begin(&target, &machine.id).is_err());
  assert_eq!(std::fs::read(target.join("deployment")).unwrap(), before);
  let rows = manager.list_non_windows(Actor::Person).await.unwrap();
  assert_eq!(rows[0].state, "Installation recovery required");
}

#[test]
fn changed_platform_and_replaced_attempt_reject_stale_completion() {
  let (_root, _manager, machine, _image, target) = fixture();
  let attempt = deployment::begin(&target, &machine.id).unwrap();
  let original = std::fs::read(target.join("platform")).unwrap();
  let receipt = std::fs::read(target.join("deployment")).unwrap();
  let mut value: serde_json::Value = serde_json::from_slice(&original).unwrap();
  value["build"] = serde_json::json!("foreign");
  std::fs::write(target.join("platform"), serde_json::to_vec(&value).unwrap()).unwrap();
  assert!(deployment::installed(&attempt).is_err());
  assert_eq!(std::fs::read(target.join("deployment")).unwrap(), receipt);
  std::fs::write(target.join("platform"), original).unwrap();
  let mut replaced: serde_json::Value = serde_json::from_slice(&receipt).unwrap();
  replaced["attempt"] = serde_json::json!(model::new_uuid());
  let bytes = serde_json::to_vec(&replaced).unwrap();
  std::fs::write(target.join("deployment"), &bytes).unwrap();
  assert!(deployment::installed(&attempt).is_err());
  assert_eq!(std::fs::read(target.join("deployment")).unwrap(), bytes);
}

#[tokio::test]
async fn written_disk_without_receipt_and_untrusted_receipts_are_rejected() {
  let (_root, manager, machine, image, target) = fixture();
  use std::io::Write;
  std::fs::OpenOptions::new()
    .write(true)
    .open(target.join("disk"))
    .unwrap()
    .write_all(b"guest data")
    .unwrap();
  assert!(deployment::begin(&target, &machine.id).is_err());
  assert!(!target.join("deployment").exists());
  assert!(platform::prepare(&manager, machine.clone(), image, Arc::new(|| Ok(()))).is_err());
  let rows = manager.list_non_windows(Actor::Person).await.unwrap();
  assert_eq!(rows[0].state, "Installation recovery required");
  std::os::unix::fs::symlink(target.join("platform"), target.join("deployment")).unwrap();
  assert!(deployment::read(&target, &machine.id).is_err());
}
