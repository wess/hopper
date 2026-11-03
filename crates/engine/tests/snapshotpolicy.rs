#![cfg(unix)]

#[allow(dead_code)]
#[path = "sessions/fixture.rs"]
mod fixture;

use engine::machines::{native::snapshot::Policy, Actor};
use fixture::{Fixture, ID};

#[test]
fn commit_uses_the_access_mutation_lock_and_releases_it_on_drop() {
  let f = Fixture::new();
  let policy = Policy::agent(f.manager.clone(), ID, 0).unwrap();
  let commit = policy.commit().unwrap();
  assert!(f.manager.set_agent_access(ID, false).is_err());
  assert!(f.manager.machine(ID, Actor::Agent).unwrap().agent_access);
  assert_eq!(
    f.manager
      .machine(ID, Actor::Person)
      .unwrap()
      .agent_generation,
    0
  );
  drop(commit);
  f.manager.set_agent_access(ID, false).unwrap();
  assert!(policy.check().is_err());
  assert!(policy.commit().is_err());
}

#[test]
fn another_process_cannot_mutate_policy_until_commit_releases() {
  let f = Fixture::new();
  let policy = Policy::agent(f.manager.clone(), ID, 0).unwrap();
  let commit = policy.commit().unwrap();
  let mutate = |busy: bool| {
    assert!(std::process::Command::new(std::env::current_exe().unwrap())
      .args(["--exact", "policy_mutator_process", "--ignored"])
      .env("HOPPER_SNAPSHOT_POLICY_ROOT", &f.manager.root)
      .env("HOPPER_SNAPSHOT_POLICY_BUSY", if busy { "1" } else { "0" })
      .status()
      .unwrap()
      .success());
  };
  mutate(true);
  policy.check().unwrap();
  drop(commit);
  mutate(false);
  assert!(policy.check().is_err());
  assert_eq!(
    f.manager
      .machine(ID, Actor::Person)
      .unwrap()
      .agent_generation,
    1
  );
}

#[test]
#[ignore = "invoked by another_process_cannot_mutate_policy_until_commit_releases"]
fn policy_mutator_process() {
  let manager = engine::machines::Machines {
    root: std::env::var_os("HOPPER_SNAPSHOT_POLICY_ROOT")
      .unwrap()
      .into(),
  };
  let busy = std::env::var("HOPPER_SNAPSHOT_POLICY_BUSY").unwrap() == "1";
  assert_eq!(manager.set_agent_access(ID, false).is_err(), busy);
  assert_eq!(
    manager.machine(ID, Actor::Person).unwrap().agent_access,
    busy
  );
}

#[test]
fn off_on_rejects_original_generation_even_without_an_intermediate_check() {
  let f = Fixture::new();
  let policy = Policy::agent(f.manager.clone(), ID, 0).unwrap();
  f.manager.set_agent_access(ID, false).unwrap();
  f.manager.set_agent_access(ID, true).unwrap();
  assert!(Policy::agent(f.manager.clone(), ID, 0).is_err());
  assert!(policy.commit().is_err());
  let current = Policy::agent(f.manager.clone(), ID, 2).unwrap();
  current.check().unwrap();
  drop(current.commit().unwrap());
}

#[test]
fn observed_invalid_record_is_permanent_across_cloned_checks() {
  let f = Fixture::new();
  let policy = Policy::agent(f.manager.clone(), ID, 0).unwrap();
  let copy = policy.clone();
  let path = f.manager.root.join("records").join(format!("{ID}.json"));
  let saved = std::fs::read(&path).unwrap();
  std::fs::write(&path, b"{}").unwrap();
  assert!(policy.check().is_err());
  std::fs::write(&path, saved).unwrap();
  assert!(copy.check().is_err());
  assert!(copy.commit().is_err());
  drop(
    Policy::agent(f.manager.clone(), ID, 0)
      .unwrap()
      .commit()
      .unwrap(),
  );
}

#[test]
fn local_person_policy_does_not_depend_on_agent_access() {
  let f = Fixture::new();
  let policy = Policy::person(f.manager.clone(), ID).unwrap();
  f.manager.set_agent_access(ID, false).unwrap();
  policy.check().unwrap();
  drop(policy.commit().unwrap());
  assert!(Policy::agent(f.manager.clone(), ID, 1).is_err());
}

#[test]
fn every_check_reads_through_trailing_buffers_and_invalidates_clones() {
  let f = Fixture::new();
  let path = f.manager.root.join("records").join(format!("{ID}.json"));
  let mut padded = std::fs::read(&path).unwrap();
  padded.extend_from_slice(&vec![b' '; 32768]);
  std::fs::write(&path, &padded).unwrap();
  let policy = Policy::agent(f.manager.clone(), ID, 0).unwrap();
  let copy = policy.clone();
  policy.check().unwrap();
  padded.push(b'x');
  std::fs::write(&path, &padded).unwrap();
  assert!(policy.check().is_err());
  padded.pop();
  std::fs::write(&path, padded).unwrap();
  assert!(copy.check().is_err());
  Policy::agent(f.manager.clone(), ID, 0)
    .unwrap()
    .check()
    .unwrap();
}

#[test]
fn foreign_identity_and_changed_runtime_reject() {
  let f = Fixture::new();
  assert!(Policy::person(f.manager.clone(), "00000000-0000-0000-0000-000000000000").is_err());
  assert!(Policy::person(f.manager.clone(), "8197E0F0-0603-43e9-a817-eaf7ab0327af").is_err());
  let policy = Policy::agent(f.manager.clone(), ID, 0).unwrap();
  let mut record = f.manager.machine(ID, Actor::Person).unwrap();
  record.runtime = None;
  store::json::write(
    &f.manager.root.join("records").join(format!("{ID}.json")),
    &record,
  )
  .unwrap();
  assert!(policy.commit().is_err());
  assert!(Policy::person(f.manager.clone(), ID).is_err());
}

#[test]
fn oversized_symlinked_and_foreign_records_reject_without_rewriting() {
  for fault in 0..3 {
    let f = Fixture::new();
    let path = f.manager.root.join("records").join(format!("{ID}.json"));
    let saved = std::fs::read(&path).unwrap();
    let policy = Policy::agent(f.manager.clone(), ID, 0).unwrap();
    match fault {
      0 => std::fs::write(&path, vec![b' '; 1024 * 1024 + 1]).unwrap(),
      1 => {
        let target = f.root.path().join("foreign.json");
        std::fs::write(&target, &saved).unwrap();
        std::fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(target, &path).unwrap();
      }
      _ => {
        let mut record = f.manager.machine(ID, Actor::Person).unwrap();
        record.id = "01010101-0101-0101-0101-010101010101".into();
        store::json::write(&path, &record).unwrap();
      }
    }
    let before = std::fs::read(&path).unwrap();
    assert!(policy.commit().is_err());
    assert!(Policy::agent(f.manager.clone(), ID, 0).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(
      std::fs::symlink_metadata(&path)
        .unwrap()
        .file_type()
        .is_symlink(),
      fault == 1
    );
  }
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "snapshotpolicy/publication.rs"]
mod publication;
