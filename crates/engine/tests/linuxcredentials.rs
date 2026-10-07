#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

#[path = "support/credentials.rs"]
mod backend;

use engine::machines::{
  linux::{provision, records},
  Actor, Machines,
};
use model::{CreateMachine, EngineResources, GuestOs};
use std::sync::{atomic::Ordering, Arc};

#[test]
fn native_linux_credentials_are_scoped_stable_and_revocable() {
  let state = Arc::new(backend::State::default());
  keyring::set_default_credential_builder(Box::new(backend::Builder(state.clone())));
  let root = tempfile::tempdir().unwrap();
  let manager = Machines {
    root: root.path().into(),
  };
  let record = records::create(
    &manager,
    CreateMachine {
      name: "Linux credentials".into(),
      profile: "ubuntu".into(),
      resources: EngineResources {
        cpus: 2,
        memory_gib: 4,
        disk_gib: 32,
      },
      installer: None,
      agent_access: true,
    },
  )
  .unwrap();
  let first = provision::persisted(&manager, &record.id, Actor::Agent).unwrap();
  let retry = provision::persisted(&manager, &record.id.to_uppercase(), Actor::Person).unwrap();
  assert!(first.user_password == retry.user_password);
  assert!(first.administrator_password == retry.administrator_password);
  assert_eq!(state.writes.load(Ordering::SeqCst), 1);
  let linux = store::guests::key_for(&record.id, GuestOs::Linux).unwrap();
  let windows = store::guests::key(&record.id).unwrap();
  assert_ne!(linux, windows);
  assert_eq!(windows, format!("machines.windows.{}.accounts", record.id));
  assert!(!state.values.lock().unwrap().contains_key(&windows));
  manager.set_agent_access(&record.id, false).unwrap();
  let opens = state.opens.load(Ordering::SeqCst);
  assert!(provision::persisted(&manager, &record.id, Actor::Agent).is_err());
  assert_eq!(state.opens.load(Ordering::SeqCst), opens);
  assert!(provision::persisted(&manager, &record.id, Actor::Person).is_ok());
  let mut previous = record.clone();
  previous.runtime = None;
  store::json::write(
    &manager
      .root
      .join("records")
      .join(format!("{}.json", record.id)),
    &previous,
  )
  .unwrap();
  assert!(provision::persisted(&manager, &record.id, Actor::Person).is_err());
  assert!(provision::persisted(&manager, "../registry.auths", Actor::Person).is_err());
  store::json::write(
    &manager
      .root
      .join("records")
      .join(format!("{}.json", record.id)),
    &record,
  )
  .unwrap();
  state
    .values
    .lock()
    .unwrap()
    .insert(linux.clone(), b"corrupt owned guest record".to_vec());
  let error = provision::persisted(&manager, &record.id, Actor::Person)
    .err()
    .unwrap();
  assert!(!error.to_string().contains("corrupt owned guest record"));
  assert!(state.values.lock().unwrap().get(&linux).unwrap() == b"corrupt owned guest record");
  assert_eq!(state.writes.load(Ordering::SeqCst), 1);
}
