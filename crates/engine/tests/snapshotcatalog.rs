#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

#[allow(dead_code)]
#[path = "sessions/fixture.rs"]
mod fixture;

use engine::machines::native::{config, snapshot::Policy};
use fixture::{Fixture, ID};
use machine::snapshot::{
  manifest::{Binding, Layout},
  package::Stage,
};
use sha2::{Digest, Sha256};
use std::{
  fs::{File, Permissions},
  os::unix::fs::PermissionsExt,
};

fn namespace(f: &Fixture) -> std::path::PathBuf {
  let paths = config::initialize(&f.manager, ID).unwrap();
  let parent = paths.root.join("snapshots");
  std::fs::create_dir(&parent).unwrap();
  std::fs::set_permissions(&parent, Permissions::from_mode(0o700)).unwrap();
  parent
}

fn publish(parent: &std::path::Path) {
  let parent = File::open(parent).unwrap();
  let binding = Binding {
    machine: model::native::machine_identity(ID).unwrap(),
    snapshot: [2; 16],
    configuration: [3; 32],
  };
  let layout = Layout {
    cpus: 1,
    banks: [(0, 65536), (65536, 65536), (1 << 30, 65536)],
    devices: [0; 7],
  };
  let mut stage = Stage::new(&parent, binding, layout, || Ok(())).unwrap();
  let mut bytes = b"test component".to_vec();
  bytes.extend_from_slice(&Sha256::digest(&bytes));
  for record in layout.records().unwrap() {
    stage
      .write(record, || Ok(()), |out| Ok(out.write_all(&bytes)?))
      .unwrap();
  }
  drop(stage.publish(|| Ok(()), || Ok(())).unwrap());
}

#[test]
fn unprepared_namespaces_are_empty_without_creating_state() {
  let f = Fixture::new();
  let policy = Policy::agent(f.manager.clone(), ID, 0).unwrap();
  assert!(policy.inventory().unwrap().is_empty());
  assert!(!f.manager.root.join("native").exists());
  let paths = config::initialize(&f.manager, ID).unwrap();
  assert!(policy.inventory().unwrap().is_empty());
  assert!(!paths.root.join("snapshots").exists());
}

#[test]
fn discovery_survives_new_policy_without_any_worker_or_session() {
  let f = Fixture::new();
  publish(&namespace(&f));
  let read = || {
    Policy::agent(f.manager.clone(), ID, 0)
      .unwrap()
      .inventory()
      .unwrap()
  };
  for _ in 0..2 {
    let entries = read();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id, "02020202-0202-0202-0202-020202020202");
    assert_eq!(entries[0].machine_id, ID);
    assert_eq!(entries[0].configuration, "03".repeat(32));
    assert_eq!(entries[0].memory_bytes, 65536);
    assert_eq!(entries[0].cpus, 1);
  }
  assert!(std::process::Command::new(std::env::current_exe().unwrap())
    .args(["--exact", "catalog_process", "--ignored"])
    .env("HOPPER_CATALOG_ROOT", &f.manager.root)
    .status()
    .unwrap()
    .success());
}

#[test]
#[ignore = "invoked by the durable discovery test in a separate process"]
fn catalog_process() {
  let manager = engine::machines::Machines {
    root: std::env::var_os("HOPPER_CATALOG_ROOT").unwrap().into(),
  };
  let entries = Policy::agent(manager, ID, 0).unwrap().inventory().unwrap();
  assert_eq!(entries.len(), 1);
  assert_eq!(entries[0].id, "02020202-0202-0202-0202-020202020202");
  assert_eq!(entries[0].configuration, "03".repeat(32));
}

#[test]
fn stale_agent_policy_rejects_discovery_after_off_on_but_person_can_list() {
  let f = Fixture::new();
  publish(&namespace(&f));
  let original = Policy::agent(f.manager.clone(), ID, 0).unwrap();
  f.manager.set_agent_access(ID, false).unwrap();
  f.manager.set_agent_access(ID, true).unwrap();
  assert!(original.inventory().is_err());
  assert!(Policy::agent(f.manager.clone(), ID, 0).is_err());
  assert_eq!(
    Policy::person(f.manager.clone(), ID)
      .unwrap()
      .inventory()
      .unwrap()
      .len(),
    1
  );
}

#[test]
fn symlinks_at_each_native_namespace_level_reject() {
  for level in 0..3 {
    let f = Fixture::new();
    let parent = namespace(&f);
    publish(&parent);
    let target = match level {
      0 => f.manager.root.join("native"),
      1 => parent.parent().unwrap().to_owned(),
      _ => parent,
    };
    let old = f.root.path().join("preserved");
    std::fs::rename(&target, &old).unwrap();
    std::os::unix::fs::symlink(&old, &target).unwrap();
    assert!(Policy::person(f.manager.clone(), ID)
      .unwrap()
      .inventory()
      .is_err());
    assert!(old.exists());
  }
}
