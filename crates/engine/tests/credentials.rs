#[path = "support/credentials.rs"]
mod backend;

use engine::machines::{
  windows::provision::{self, Role},
  Actor, Machines,
};
use fs2::FileExt;
use model::{EngineResources, GuestOs, Machine};
use std::sync::{atomic::Ordering, Arc};

fn record(manager: &Machines, id: &str, guest: GuestOs, access: bool) {
  let machine = Machine {
    id: id.to_string(),
    name: "Credential test".into(),
    guest,
    profile: "windows".into(),
    resources: EngineResources {
      cpus: 2,
      memory_gib: 4,
      disk_gib: 64,
    },
    agent_access: access,
    installer: None,
  };
  store::json::write(
    &manager.root.join("records").join(format!("{id}.json")),
    &machine,
  )
  .unwrap();
}

#[test]
fn setup_retries_preserve_credentials_and_fail_closed_on_store_or_policy_errors() {
  let state = Arc::new(backend::State::default());
  keyring::set_default_credential_builder(Box::new(backend::Builder(state.clone())));
  let root = tempfile::tempdir().unwrap();
  let manager = Machines {
    root: root.path().into(),
  };
  let id = "8197e0f0-0603-43e9-a817-eaf7ab0327af";
  let clone = "8197e0f0-0603-43e9-a817-eaf7ab0327b0";
  record(&manager, id, GuestOs::Windows, true);
  std::fs::create_dir_all(manager.root.join("locks")).unwrap();
  let held =
    std::fs::File::create(manager.root.join("locks").join(format!("{id}.credentials"))).unwrap();
  held.lock_exclusive().unwrap();
  assert!(provision::persisted(&manager, &id.to_uppercase(), Actor::Person).is_err());
  assert_eq!(state.opens.load(Ordering::SeqCst), 0);
  drop(held);
  let first = provision::persisted(&manager, id, Actor::Agent).unwrap();
  let restarted = Machines {
    root: root.path().into(),
  };
  let retry = provision::persisted(&restarted, &id.to_uppercase(), Actor::Person).unwrap();
  for role in [Role::User, Role::Administrator] {
    assert!(provision::password(&first, role) == provision::password(&retry, role));
  }
  assert_eq!(state.writes.load(Ordering::SeqCst), 1);
  assert!(state
    .values
    .lock()
    .unwrap()
    .values()
    .all(|raw| !raw.contains(&b'\n')));
  let slot = store::guests::open(id).unwrap();
  let existing = store::guests::read(&slot).unwrap().unwrap();
  assert!(store::guests::create(&slot, &existing).is_err());
  assert_eq!(state.writes.load(Ordering::SeqCst), 1);

  record(&manager, clone, GuestOs::Windows, true);
  let cloned = provision::persisted(&manager, clone, Actor::Person).unwrap();
  assert!(provision::password(&first, Role::User) != provision::password(&cloned, Role::User));
  assert_eq!(state.values.lock().unwrap().len(), 2);
  let opens = state.opens.load(Ordering::SeqCst);
  record(&manager, id, GuestOs::Windows, false);
  assert!(provision::persisted(&manager, id, Actor::Agent).is_err());
  record(&manager, id, GuestOs::Linux, true);
  assert!(provision::persisted(&manager, id, Actor::Person).is_err());
  assert!(provision::persisted(&manager, "../registry.auths", Actor::Person).is_err());
  assert_eq!(state.opens.load(Ordering::SeqCst), opens);

  record(&manager, id, GuestOs::Windows, true);
  state.denied.store(true, Ordering::SeqCst);
  let denied = provision::persisted(&manager, id, Actor::Person)
    .err()
    .unwrap();
  assert!(!denied.to_string().contains("private backend detail"));
  assert_eq!(state.writes.load(Ordering::SeqCst), 2);
  state.denied.store(false, Ordering::SeqCst);
  let key = store::guests::key(id).unwrap();
  state
    .values
    .lock()
    .unwrap()
    .insert(key.clone(), b"corrupt private value".to_vec());
  let corrupt = provision::persisted(&manager, id, Actor::Person)
    .err()
    .unwrap();
  assert!(!corrupt.to_string().contains("corrupt private value"));
  assert_eq!(state.writes.load(Ordering::SeqCst), 2);
  assert!(state.values.lock().unwrap().get(&key).unwrap() == b"corrupt private value");
  assert_eq!(
    store::guests::key(id).unwrap(),
    store::guests::key(&id.to_uppercase()).unwrap()
  );
  assert!(store::guests::key("github.token").is_err());

  let fresh = "8197e0f0-0603-43e9-a817-eaf7ab0327b1";
  record(&manager, fresh, GuestOs::Windows, true);
  state.write_denied.store(true, Ordering::SeqCst);
  let failed = provision::persisted(&manager, fresh, Actor::Person)
    .err()
    .unwrap();
  assert_eq!(failed.to_string(), "Cannot save guest credentials");
  assert_eq!(state.writes.load(Ordering::SeqCst), 2);
  assert!(!state
    .values
    .lock()
    .unwrap()
    .contains_key(&store::guests::key(fresh).unwrap()));
  state.write_denied.store(false, Ordering::SeqCst);
  assert!(provision::persisted(&manager, fresh, Actor::Person).is_ok());
  assert_eq!(state.writes.load(Ordering::SeqCst), 3);
}
