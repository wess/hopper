use super::fixture;
use engine::machines::native::restore::Owner;
use fixture::{Fixture, ID};
use std::fs::OpenOptions;

#[test]
fn host_restore_owner_excludes_running_hardware_and_preserves_original_policy() {
  let f = Fixture::new();
  f.initialize();
  let path = f.manager.root.join("locks").join(format!("{ID}.runtime"));
  std::fs::create_dir_all(path.parent().unwrap()).unwrap();
  let open = || {
    OpenOptions::new()
      .read(true)
      .write(true)
      .create(true)
      .truncate(false)
      .open(&path)
      .unwrap()
  };
  let runtime = store::lock::exclusive(open()).unwrap();
  assert!(Owner::agent(f.manager.clone(), ID, 0).is_err());
  drop(runtime);
  let owner = Owner::agent(f.manager.clone(), ID, 0).unwrap();
  assert!(owner.selected().unwrap().is_none());
  assert!(Owner::person(f.manager.clone(), ID).is_err());
  assert!(store::lock::exclusive(open()).is_err());
  f.manager.set_agent_access(ID, false).unwrap();
  f.manager.set_agent_access(ID, true).unwrap();
  assert!(owner.check().is_err());
  assert!(owner.selected().is_err());
  assert!(store::lock::exclusive(open()).is_err());
  drop(owner);
  drop(store::lock::exclusive(open()).unwrap());
  assert!(Owner::agent(f.manager.clone(), ID, 0).is_err());
  drop(Owner::person(f.manager.clone(), ID).unwrap());
}
