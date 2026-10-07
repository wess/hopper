#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use engine::machines::{
  vz::{audio, records},
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
      name: "Speaker fixture".into(),
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
fn default_speakers_and_private_muted_policy_survive_reopening_without_allocating_hardware() {
  for profile in ["ubuntu", "macos"] {
    let (_root, manager, machine) = fixture(profile);
    let id = &machine.id;
    let record = manager.root.join("records").join(format!("{id}.json"));
    let original = std::fs::read(&record).unwrap();
    assert!(audio::speakers(&manager, id, Actor::Person).unwrap());
    assert!(!manager.root.join("audio").exists());
    audio::set_speakers(&manager, id, false).unwrap();
    let reopened = Machines {
      root: manager.root.clone(),
    };
    assert!(!audio::speakers(&reopened, id, Actor::Agent).unwrap());
    let directory = manager.root.join("audio");
    assert_eq!(std::fs::metadata(&directory).unwrap().mode() & 0o777, 0o700);
    assert_eq!(
      std::fs::metadata(directory.join(id)).unwrap().mode() & 0o777,
      0o600
    );
    audio::set_speakers(&manager, id, true).unwrap();
    assert!(audio::speakers(&reopened, id, Actor::Person).unwrap());
    assert_eq!(std::fs::read(record).unwrap(), original);
    assert!(!manager.root.join("vz").exists());
    manager.set_agent_access(id, false).unwrap();
    assert!(audio::speakers(&manager, id, Actor::Agent).is_err());
  }
}

#[test]
fn active_operation_and_runtime_ownership_exclude_audio_changes() {
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
    assert!(audio::set_speakers(&manager, &machine.id, false).is_err());
    assert!(audio::speakers(&manager, &machine.id, Actor::Person).unwrap());
    assert!(!manager.root.join("audio").exists());
    FileExt::unlock(&lock).unwrap();
  }
  audio::set_speakers(&manager, &machine.id, false).unwrap();
}

#[test]
fn invalid_and_foreign_muted_settings_never_enable_speakers_or_get_replaced() {
  let (_root, manager, machine) = fixture("macos");
  let id = &machine.id;
  audio::set_speakers(&manager, id, false).unwrap();
  let path = manager.root.join("audio").join(id);
  for data in [
    b"broken".to_vec(),
    serde_json::to_vec(&serde_json::json!({"version":2,"id":id,"speakers":false})).unwrap(),
    serde_json::to_vec(&serde_json::json!({"version":1,"id":model::new_uuid(),"speakers":false}))
      .unwrap(),
    serde_json::to_vec(
      &serde_json::json!({"version":1,"id":id,"speakers":false,"microphone":true}),
    )
    .unwrap(),
    vec![b' '; 1025],
  ] {
    std::fs::write(&path, &data).unwrap();
    assert!(audio::speakers(&manager, id, Actor::Person).is_err());
    assert!(audio::set_speakers(&manager, id, true).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), data);
  }
  let outside = manager.root.join("outside");
  std::fs::rename(&path, &outside).unwrap();
  std::os::unix::fs::symlink(&outside, &path).unwrap();
  assert!(audio::speakers(&manager, id, Actor::Person).is_err());
  assert!(audio::set_speakers(&manager, id, true).is_err());
  assert!(path.is_symlink());
  assert_eq!(std::fs::read(outside).unwrap(), vec![b' '; 1025]);
}

#[test]
fn previous_and_unsupported_guests_reject_audio_updates_without_changing_disks() {
  let (_root, manager, mut machine) = fixture("ubuntu");
  let previous = manager.root.join("lima").join(&machine.id);
  std::fs::create_dir_all(&previous).unwrap();
  let disk = previous.join("diffdisk");
  std::fs::write(&disk, b"preserved").unwrap();
  assert!(audio::set_speakers(&manager, &machine.id, false).is_err());
  assert!(!manager.root.join("audio").exists());
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
    assert!(audio::speakers(&manager, &machine.id, Actor::Person).is_err());
    assert!(audio::set_speakers(&manager, &machine.id, false).is_err());
    assert!(!manager.root.join("audio").exists());
  }
}
