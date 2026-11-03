#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[allow(dead_code)]
#[path = "restore/active.rs"]
mod active;
#[allow(dead_code)]
#[path = "restore/fixture.rs"]
mod fixture;
use active::{active, boots};
use engine::machines::{
  native::{config, restore::Owner, sessions::Sessions, State},
  Actor,
};
use fixture::{Fixture, ID};
use model::native::{Command, StopReason};
use std::{fs, time::Duration};

async fn until(mut ready: impl FnMut() -> bool) {
  tokio::time::timeout(Duration::from_secs(4), async {
    while !ready() {
      tokio::time::sleep(Duration::from_millis(5)).await;
    }
  })
  .await
  .unwrap();
}

fn path(f: &Fixture) -> config::Paths {
  config::paths(&f.manager.root, ID).unwrap()
}

async fn event(f: &Fixture, sessions: &Sessions, reason: &str) {
  fs::write(path(f).variables.join("powerevent"), reason).unwrap();
  let _ = sessions
    .request(ID, Actor::Person, Command::Status {})
    .await;
}

async fn start(f: &Fixture) -> Sessions {
  let sessions = Sessions::new(f.manager.clone());
  sessions
    .start_configured(ID, &f.assets(), config::Stage::System)
    .await
    .unwrap();
  sessions
}

#[tokio::test]
async fn guest_reset_reboots_active_generation_and_keeps_the_viewer_watch() {
  let f = active().await;
  let root = path(&f);
  fs::rename(&root.disk, root.disk.with_extension("saved")).unwrap();
  let receipt = fs::read(root.root.join("restore.hrr")).unwrap();
  let sessions = start(&f).await;
  let watch = sessions.watch(ID).await.unwrap();
  for expected in [3, 4] {
    event(&f, &sessions, "reset").await;
    until(|| boots(&f).lines().count() == expected && matches!(*watch.borrow(), State::Running))
      .await;
  }
  assert!(!root.disk.exists());
  assert_eq!(fs::read(root.root.join("restore.hrr")).unwrap(), receipt);
  sessions.stop(ID, Actor::Person).await.unwrap();
  assert!(matches!(
    *watch.borrow(),
    State::Stopped(StopReason::Requested)
  ));
  tokio::time::sleep(Duration::from_millis(250)).await;
  assert_eq!(boots(&f).lines().count(), 4);
  drop(Owner::person(f.manager.clone(), ID).unwrap());
}

#[tokio::test]
async fn guest_shutdown_and_requested_stop_do_not_restart() {
  for shutdown in [false, true] {
    let f = active().await;
    let sessions = start(&f).await;
    if shutdown {
      event(&f, &sessions, "shutdown").await;
      let watch = sessions.watch(ID).await.unwrap();
      until(|| matches!(*watch.borrow(), State::Stopped(StopReason::Shutdown))).await;
    } else {
      sessions.stop(ID, Actor::Person).await.unwrap();
    }
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert_eq!(boots(&f).lines().count(), 2);
    drop(sessions);
    until(|| Owner::person(f.manager.clone(), ID).is_ok()).await;
  }
}

#[tokio::test]
async fn failed_reboot_preserves_active_and_allows_explicit_retry() {
  let f = active().await;
  let root = path(&f);
  let receipt = fs::read(root.root.join("restore.hrr")).unwrap();
  let sessions = start(&f).await;
  fs::write(root.variables.join("systemreject"), []).unwrap();
  event(&f, &sessions, "reset").await;
  until(|| boots(&f).lines().count() == 3 && Owner::person(f.manager.clone(), ID).is_ok()).await;
  assert_eq!(fs::read(root.root.join("restore.hrr")).unwrap(), receipt);
  fs::remove_file(root.variables.join("systemreject")).unwrap();
  sessions
    .start_configured(ID, &f.assets(), config::Stage::System)
    .await
    .unwrap();
  assert_eq!(boots(&f).lines().count(), 4);
  sessions.stop(ID, Actor::Person).await.unwrap();
}

#[tokio::test]
async fn registry_drop_during_reboot_cancels_and_retains_runtime_until_reaped() {
  let f = active().await;
  let sessions = start(&f).await;
  // hold startup after the next worker has received its generation request.
  fs::write(path(&f).variables.join("generationgate"), []).unwrap();
  event(&f, &sessions, "reset").await;
  until(|| boots(&f).lines().count() == 3).await;
  assert!(Owner::person(f.manager.clone(), ID).is_err());
  drop(sessions);
  until(|| Owner::person(f.manager.clone(), ID).is_ok()).await;
  assert_eq!(boots(&f).lines().count(), 3);
}
