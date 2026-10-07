#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

#[path = "support/mac.rs"]
mod support;
use engine::machines::{
  vz::{
    self,
    mac::{deployment, Phase},
  },
  Actor,
};
use support::fixture;

#[tokio::test]
async fn setup_cancel_invalidates_prepared_launch_while_its_operation_lock_is_held() {
  let (_root, manager, machine, _image, target) = fixture();
  deployment::installed(&deployment::begin(&target, &machine.id).unwrap()).unwrap();
  let (client, _owner) = vz::channel();
  let service = vz::Service::new(manager.clone(), client);
  let (progress, updates) = tokio::sync::watch::channel(Phase::Inspecting);
  let launch = service
    .prepare_mac(&machine.id, Actor::Person, progress)
    .await
    .unwrap();
  assert_eq!(*updates.borrow(), Phase::Preparing);
  launch.authorized().unwrap();
  service.cancel_mac(&machine.id, Actor::Person).unwrap();
  assert!(launch
    .authorized()
    .unwrap_err()
    .to_string()
    .contains("explicit stop"));
  assert_eq!(
    deployment::read(&target, &machine.id).unwrap(),
    Some(deployment::Phase::Installed)
  );
}

#[tokio::test]
async fn prepared_launch_does_not_refresh_revoked_agent_authorization() {
  let (_root, manager, machine, _image, target) = fixture();
  deployment::installed(&deployment::begin(&target, &machine.id).unwrap()).unwrap();
  let (client, _owner) = vz::channel();
  let service = vz::Service::new(manager.clone(), client);
  let (progress, _) = tokio::sync::watch::channel(Phase::Inspecting);
  let launch = service
    .prepare_mac(&machine.id, Actor::Agent, progress)
    .await
    .unwrap();
  manager.set_agent_access(&machine.id, false).unwrap();
  manager.set_agent_access(&machine.id, true).unwrap();
  assert!(launch
    .authorized()
    .unwrap_err()
    .to_string()
    .contains("policy"));
}

#[test]
fn revoked_or_previous_runtime_cancel_cannot_write_a_stop_intent() {
  let (_root, manager, machine, _image, target) = fixture();
  let (client, _owner) = vz::channel();
  let service = vz::Service::new(manager.clone(), client);
  manager.set_agent_access(&machine.id, false).unwrap();
  assert!(service.cancel_mac(&machine.id, Actor::Agent).is_err());
  assert!(!manager.root.join("intent").exists());
  let previous = manager.root.join("lima").join(&machine.id);
  std::fs::create_dir_all(&previous).unwrap();
  assert!(service.cancel_mac(&machine.id, Actor::Person).is_err());
  assert!(!manager.root.join("intent").exists());
  assert!(target.join("disk").exists());
}
