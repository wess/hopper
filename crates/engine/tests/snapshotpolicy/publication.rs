use super::{Actor, Fixture, Policy, ID};
use machine::snapshot::{
  manifest::{Binding, Layout},
  package::{Package, Stage},
};
use sha2::{Digest, Sha256};
use std::{cell::Cell, fs::File, os::unix::fs::PermissionsExt};

fn binding() -> Binding {
  Binding {
    machine: model::native::machine_identity(ID).unwrap(),
    snapshot: [2; 16],
    configuration: [3; 32],
  }
}

fn layout() -> Layout {
  Layout {
    cpus: 1,
    banks: [(0, 65536), (65536, 65536), (1 << 30, 65536)],
    devices: [0; 7],
  }
}

fn stage(f: &Fixture, policy: &Policy) -> (File, Stage) {
  let path = f.root.path().join("packages");
  std::fs::create_dir(&path).unwrap();
  std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
  let parent = File::open(path).unwrap();
  let mut stage = Stage::new(&parent, binding(), layout(), || policy.check()).unwrap();
  let mut bytes = b"test component".to_vec();
  bytes.extend_from_slice(&Sha256::digest(&bytes));
  for record in layout().records().unwrap() {
    stage
      .write(record, || policy.check(), |out| Ok(out.write_all(&bytes)?))
      .unwrap();
  }
  (parent, stage)
}

#[test]
fn package_publication_holds_saved_policy_through_completion() {
  let f = Fixture::new();
  let policy = Policy::agent(f.manager.clone(), ID, 0).unwrap();
  let (parent, stage) = stage(&f, &policy);
  let checks = Cell::new(0);
  let package = stage
    .publish(
      || {
        checks.set(checks.get() + 1);
        policy.check()?;
        if checks.get() >= 2 {
          assert!(f.manager.set_agent_access(ID, false).is_err());
          assert_eq!(f.manager.machine(ID, Actor::Agent)?.agent_generation, 0);
        }
        Ok(())
      },
      || policy.commit(),
    )
    .unwrap();
  assert_eq!(checks.get(), 3);
  assert!(Package::open(&parent, binding(), layout(), || policy.check()).is_ok());
  drop(package);
  f.manager.set_agent_access(ID, false).unwrap();
  // completed packages remain intact, while the original agent loses read authorization.
  assert!(Package::open(&parent, binding(), layout(), || policy.check()).is_err());
  assert!(Package::open(&parent, binding(), layout(), || Ok(())).is_ok());
}

#[test]
fn saved_policy_toggle_between_serialization_and_commit_prevents_rename() {
  let f = Fixture::new();
  let policy = Policy::agent(f.manager.clone(), ID, 0).unwrap();
  let (parent, stage) = stage(&f, &policy);
  let result = stage.publish(
    || policy.check(),
    || {
      f.manager.set_agent_access(ID, false)?;
      f.manager.set_agent_access(ID, true)?;
      policy.commit()
    },
  );
  assert!(result.is_err());
  assert_eq!(
    f.manager
      .machine(ID, Actor::Agent)
      .unwrap()
      .agent_generation,
    2
  );
  assert!(Package::open(&parent, binding(), layout(), || Ok(())).is_err());
  let entries: Vec<_> = std::fs::read_dir(f.root.path().join("packages"))
    .unwrap()
    .map(|entry| entry.unwrap().file_name())
    .collect();
  assert_eq!(entries.len(), 1);
  assert!(entries[0].to_string_lossy().starts_with(".stage"));
  // failure must not leak the access lock or authorize replacing the abandoned stage.
  f.manager.set_agent_access(ID, false).unwrap();
}

#[test]
fn host_commits_a_reopened_worker_stage_under_original_policy() {
  let f = Fixture::new();
  let policy = Policy::agent(f.manager.clone(), ID, 0).unwrap();
  let (parent, stage) = stage(&f, &policy);
  let sealed = stage.seal(|| policy.check()).unwrap();
  let receipt = serde_json::from_slice(&serde_json::to_vec(&sealed.receipt()).unwrap()).unwrap();
  drop(sealed);
  assert!(Package::open(&parent, binding(), layout(), || Ok(())).is_err());
  let mut foreign = binding();
  foreign.machine = [8; 16];
  assert!(policy.publish(&parent, receipt, foreign, layout()).is_err());
  drop(
    policy
      .publish(&parent, receipt, binding(), layout())
      .unwrap(),
  );
  assert!(Package::open(&parent, binding(), layout(), || policy.check()).is_ok());
  assert!(policy
    .publish(&parent, receipt, binding(), layout())
    .is_err());
  f.manager.set_agent_access(ID, false).unwrap();
}

#[test]
fn host_rejects_a_sealed_stage_after_policy_toggle() {
  let f = Fixture::new();
  let policy = Policy::agent(f.manager.clone(), ID, 0).unwrap();
  let (parent, stage) = stage(&f, &policy);
  let sealed = stage.seal(|| policy.check()).unwrap();
  let receipt = sealed.receipt();
  drop(sealed);
  f.manager.set_agent_access(ID, false).unwrap();
  f.manager.set_agent_access(ID, true).unwrap();
  assert!(policy
    .publish(&parent, receipt, binding(), layout())
    .is_err());
  assert!(Package::open(&parent, binding(), layout(), || Ok(())).is_err());
}
