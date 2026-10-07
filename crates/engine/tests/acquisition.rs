#![cfg(unix)]

#[allow(dead_code)]
#[path = "media/fixture.rs"]
mod cache;
#[allow(dead_code)]
#[path = "setup/fixture.rs"]
mod setup;
#[allow(dead_code)]
#[path = "startup/fixture.rs"]
mod startup;

use engine::machines::{
  native::{
    deployment::{Phase, Tools},
    sessions::Sessions,
  },
  Actor,
};
use std::{os::unix::fs::PermissionsExt, sync::Arc, time::Duration};
use tokio::sync::watch;

#[tokio::test]
async fn native_acquisition_pins_verified_cache_without_reverting_agent_policy() {
  keyring::set_default_credential_builder(keyring::mock::default_credential_builder());
  let f = startup::Fixture::new();
  let cached = cache::Fixture::new();
  let iso = cached.seed(&f.manager.root).await;
  let paths = engine::machines::native::config::initialize(&f.manager, startup::ID).unwrap();
  std::fs::create_dir(&paths.variables).unwrap();
  let source = setup::Fixture::new("normal");
  let mount = source.root.path().join("mount");
  std::fs::write(&mount, include_str!("inspect/tool.py")).unwrap();
  std::fs::set_permissions(&mount, std::fs::Permissions::from_mode(0o700)).unwrap();
  let wrapper = cached.root.path().join("wrapper");
  std::fs::write(
    cached.root.path().join("original"),
    source.tools.archive.to_str().unwrap(),
  )
  .unwrap();
  std::fs::write(&wrapper, include_str!("media/wrapper.py")).unwrap();
  std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700)).unwrap();
  let input = source.input();
  let mut media = source.tools.clone();
  media.archive = wrapper;
  let tools = Tools {
    media,
    mount,
    drivers: input.drivers.into(),
    license: input.license.into(),
  };
  let mut record = f.manager.machine(startup::ID, Actor::Person).unwrap();
  record.installer = None;
  store::json::write(&f.record(), &record).unwrap();
  std::fs::write(cached.root.path().join("gate"), "").unwrap();
  let sessions = Arc::new(Sessions::new(f.manager.clone()));
  let (progress, status) = watch::channel(Phase::Inspecting);
  let owner = sessions.clone();
  let assets = f.assets();
  let copied = tools.clone();
  let report = progress.clone();
  let task = tokio::spawn(async move { owner.deploy(startup::ID, &assets, &copied, report).await });
  tokio::time::timeout(Duration::from_secs(3), async {
    while !cached.root.path().join("waiting").exists() {
      tokio::time::sleep(Duration::from_millis(5)).await;
    }
  })
  .await
  .unwrap();
  assert!(matches!(*status.borrow(), Phase::Acquiring(_)));
  f.manager.set_agent_access(startup::ID, false).unwrap();
  std::fs::write(cached.root.path().join("continue"), "").unwrap();
  task.await.unwrap().unwrap();
  let record = f.manager.machine(startup::ID, Actor::Person).unwrap();
  assert_eq!(record.installer.as_deref(), iso.to_str());
  assert!(!record.agent_access);
  assert!(sessions.state(startup::ID, Actor::Agent).await.is_err());
  sessions.stop(startup::ID, Actor::Person).await.unwrap();
  // retries must verify the native cache even after its path was saved in the record.
  std::fs::write(
    iso.parent().unwrap().join("catalogue.cab"),
    b"modified source",
  )
  .unwrap();
  assert!(sessions
    .deploy(startup::ID, &f.assets(), &tools, progress)
    .await
    .is_err());
  assert!(sessions
    .state(startup::ID, Actor::Person)
    .await
    .unwrap()
    .is_none());
  assert!(
    !f.manager
      .machine(startup::ID, Actor::Person)
      .unwrap()
      .agent_access
  );
  assert_eq!(std::fs::read(iso).unwrap(), [1; 32]);
}
