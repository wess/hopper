#![cfg(unix)]

#[path = "startup/fixture.rs"]
mod fixture;

use engine::machines::{
  native::{
    assets,
    config::{self, Stage},
    sessions::Sessions,
  },
  Actor,
};
use fixture::{Fixture, ID};
use fs2::FileExt;
use model::native::{Command, Result as Reply};
use std::{
  io::{Read, Seek, SeekFrom, Write},
  os::unix::fs::PermissionsExt,
  path::Path,
};

#[test]
fn packaged_paths_are_coherent_for_app_and_sidecars() {
  for exe in [
    "/Applications/Hopper.app/Contents/MacOS/Hopper",
    "/Applications/Hopper.app/Contents/MacOS/sidecars/hoppermcp",
  ] {
    let (helper, firmware) = assets::candidates(Path::new(exe)).unwrap();
    assert_eq!(
      helper,
      Path::new("/Applications/Hopper.app/Contents/MacOS/sidecars/hoppervm")
    );
    assert_eq!(
      firmware,
      Path::new("/Applications/Hopper.app/Contents/Resources/firmware")
    );
  }
  for exe in [
    "/build/debug/hopperdev",
    "/build/debug/examples/startup",
    "/build/debug/deps/test",
  ] {
    assert_eq!(
      assets::candidates(Path::new(exe)).unwrap().0,
      Path::new("/build/debug/hoppervm")
    );
  }
  assert!(assets::candidates(Path::new("relative/hopperdev")).is_err());
}

#[test]
fn artifact_verification_rejects_tampering_symlinks_and_non_executable_workers() {
  let f = Fixture::new();
  assert_eq!(assets::worker(&f.assets()), &f.helper);
  std::fs::write(f.firmware.join("windows.fd"), [3; 32]).unwrap();
  assert!(assets::verify(&f.helper, &f.firmware).is_err());
  std::fs::write(f.firmware.join("windows.fd"), [1; 32]).unwrap();
  std::fs::rename(f.firmware.join("variables.fd"), f.firmware.join("original")).unwrap();
  std::os::unix::fs::symlink(f.firmware.join("original"), f.firmware.join("variables.fd")).unwrap();
  assert!(assets::verify(&f.helper, &f.firmware).is_err());
  std::fs::remove_file(f.firmware.join("variables.fd")).unwrap();
  std::fs::rename(f.firmware.join("original"), f.firmware.join("variables.fd")).unwrap();
  std::fs::set_permissions(&f.helper, std::fs::Permissions::from_mode(0o600)).unwrap();
  assert!(assets::verify(&f.helper, &f.firmware).is_err());
}

#[test]
fn manifest_bounds_and_truncated_artifacts_are_rejected() {
  let f = Fixture::new();
  std::fs::write(f.firmware.join("variables.fd"), [2; 15]).unwrap();
  assert!(assets::verify(&f.helper, &f.firmware).is_err());
  std::fs::write(f.firmware.join("variables.fd"), [2; 16]).unwrap();
  std::fs::write(
    f.firmware.join("manifest.json"),
    vec![b' '; 1024 * 1024 + 1],
  )
  .unwrap();
  assert!(assets::verify(&f.helper, &f.firmware).is_err());
}

#[test]
fn boot_paths_and_identity_follow_the_record_and_system_boot_detaches_media() {
  let f = Fixture::new();
  let paths = f.initialize();
  let boot = config::prepare(&f.manager, ID, &f.assets(), Stage::Deployment).unwrap();
  assert_eq!(boot.disk.as_deref(), paths.disk.to_str());
  assert_eq!(boot.store, paths.variables.to_str().unwrap());
  assert_eq!(boot.boot_media.as_deref(), paths.setup.to_str());
  assert!(boot.installer.is_some());
  assert_eq!(
    (boot.cpus, boot.memory, boot.timeout_ms),
    (2, 2 * 1024 * 1024 * 1024, None)
  );
  assert_eq!(boot.disk_id.len(), 20);
  assert!(config::prepare(&f.manager, ID, &f.assets(), Stage::System).is_err());
  std::fs::OpenOptions::new()
    .write(true)
    .open(&paths.disk)
    .unwrap()
    .write_all(b"synthetic deployed disk")
    .unwrap();
  let system = config::prepare(&f.manager, ID, &f.assets(), Stage::System).unwrap();
  assert_eq!(boot.disk_id, system.disk_id);
  assert!(system.boot_media.is_none() && system.installer.is_none());
  assert!(config::paths(&f.manager.root, &ID.to_uppercase()).is_err());
  assert!(config::paths(&f.manager.root, "../host").is_err());
}

#[test]
fn deployment_preserves_written_targets_and_locked_disks() {
  let f = Fixture::new();
  let paths = f.initialize();
  let mut file = std::fs::OpenOptions::new()
    .read(true)
    .write(true)
    .open(&paths.disk)
    .unwrap();
  file.try_lock_exclusive().unwrap();
  assert!(config::prepare(&f.manager, ID, &f.assets(), Stage::Deployment).is_err());
  FileExt::unlock(&file).unwrap();
  file.seek(SeekFrom::Start(1024 * 1024)).unwrap();
  file.write_all(b"existing guest data").unwrap();
  assert!(
    config::prepare(&f.manager, ID, &f.assets(), Stage::Deployment)
      .unwrap_err()
      .to_string()
      .contains("allocated data")
  );
  assert!(config::prepare(&f.manager, ID, &f.assets(), Stage::System).is_ok());
  file.seek(SeekFrom::Start(1024 * 1024)).unwrap();
  let mut saved = [0; 19];
  file.read_exact(&mut saved).unwrap();
  assert_eq!(&saved, b"existing guest data");
}

#[test]
fn private_storage_and_resource_bounds_fail_without_allocating_a_vm() {
  let f = Fixture::new();
  let mut machine = f.manager.machine(ID, Actor::Person).unwrap();
  machine.resources.cpus = 3;
  store::json::write(&f.record(), &machine).unwrap();
  assert!(config::initialize(&f.manager, ID).is_err());
  assert!(!f.manager.root.join("native").exists());
  machine.resources.cpus = 2;
  store::json::write(&f.record(), &machine).unwrap();
  let paths = f.initialize();
  std::fs::set_permissions(&paths.root, std::fs::Permissions::from_mode(0o755)).unwrap();
  assert!(config::prepare(&f.manager, ID, &f.assets(), Stage::System).is_err());
  std::fs::set_permissions(&paths.root, std::fs::Permissions::from_mode(0o700)).unwrap();
  std::fs::set_permissions(&paths.disk, std::fs::Permissions::from_mode(0o644)).unwrap();
  assert!(config::prepare(&f.manager, ID, &f.assets(), Stage::System).is_err());
}

#[tokio::test]
async fn configured_startup_reaches_the_worker_under_registry_ownership() {
  let f = Fixture::new();
  let paths = f.initialize();
  std::fs::OpenOptions::new()
    .write(true)
    .open(&paths.disk)
    .unwrap()
    .write_all(b"synthetic deployed disk")
    .unwrap();
  std::fs::create_dir(&paths.variables).unwrap();
  let sessions = Sessions::new(f.manager.clone());
  sessions
    .start_configured(ID, &f.assets(), Stage::System)
    .await
    .unwrap();
  assert!(matches!(
    sessions
      .request(ID, Actor::Person, Command::Status {})
      .await
      .unwrap(),
    Reply::Status { .. }
  ));
  sessions.stop(ID, Actor::Person).await.unwrap();
  assert_eq!(
    std::fs::read_to_string(paths.variables.join("trace")).unwrap(),
    "status\nstop\n"
  );
}

#[test]
fn symlinked_vm_storage_never_creates_files_in_the_target() {
  let f = Fixture::new();
  let outside = f.root.path().join("outside");
  std::fs::create_dir(&outside).unwrap();
  std::os::unix::fs::symlink(&outside, f.manager.root.join("native")).unwrap();
  assert!(config::initialize(&f.manager, ID).is_err());
  assert_eq!(outside.read_dir().unwrap().count(), 0);
}

#[test]
fn independent_identities_get_distinct_storage_and_disk_serials() {
  let f = Fixture::new();
  let initial = f.initialize();
  std::fs::OpenOptions::new()
    .write(true)
    .open(&initial.disk)
    .unwrap()
    .write_all(b"synthetic deployed disk")
    .unwrap();
  let first = config::prepare(&f.manager, ID, &f.assets(), Stage::System).unwrap();
  let mut machine = f.manager.machine(ID, Actor::Person).unwrap();
  machine.id = "8197e0f0-0603-43e9-a817-eaf7ab0327ae".into();
  store::json::write(
    &f.manager
      .root
      .join("records")
      .join(format!("{}.json", machine.id)),
    &machine,
  )
  .unwrap();
  let paths = config::initialize(&f.manager, &machine.id).unwrap();
  let mut options = std::fs::OpenOptions::new();
  use std::os::unix::fs::OpenOptionsExt;
  let mut disk = options
    .write(true)
    .create_new(true)
    .mode(0o600)
    .open(paths.disk)
    .unwrap();
  disk.set_len(64 * 1024 * 1024 * 1024).unwrap();
  disk.write_all(b"synthetic deployed disk").unwrap();
  let second = config::prepare(&f.manager, &machine.id, &f.assets(), Stage::System).unwrap();
  assert_ne!(first.disk_id, second.disk_id);
  assert_ne!(first.store, second.store);
  assert_ne!(first.disk, second.disk);
}

#[test]
fn setup_media_scope_and_checksum_are_checked_before_deployment() {
  let f = Fixture::new();
  let paths = f.initialize();
  let metadata = paths.setup.with_extension("json");
  let original = std::fs::read(&metadata).unwrap();
  let mut manifest: serde_json::Value = serde_json::from_slice(&original).unwrap();
  manifest["vmId"] = "8197e0f0-0603-43e9-a817-eaf7ab0327ae".into();
  std::fs::write(&metadata, serde_json::to_vec(&manifest).unwrap()).unwrap();
  assert!(config::prepare(&f.manager, ID, &f.assets(), Stage::Deployment).is_err());
  std::fs::write(&metadata, original).unwrap();
  std::fs::write(&paths.setup, [3; 32]).unwrap();
  assert!(config::prepare(&f.manager, ID, &f.assets(), Stage::Deployment).is_err());
}
