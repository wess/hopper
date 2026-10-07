#![cfg(unix)]

#[path = "sessions/fixture.rs"]
mod fixture;

use engine::machines::{
  native::{sessions::Sessions, State},
  Actor,
};
use fixture::{input, wait_file, wait_lock, Fixture, ID};
use fs2::FileExt;
use model::native::{Command, Result as Reply};
use std::time::Duration;

#[tokio::test]
async fn manager_retains_sessions_and_rechecks_persisted_policy() {
  let f = Fixture::new();
  f.start("normal").await;
  drop(f.sessions.watch(ID).await.unwrap());
  assert_eq!(
    f.sessions.state(ID, Actor::Person).await.unwrap(),
    Some(State::Running)
  );
  let other = Sessions::new(f.manager.clone());
  assert!(other.start(ID, &f.helper, f.boot("normal")).await.is_err());
  f.manager.set_agent_access(ID, false).unwrap();
  assert!(f.sessions.capture(ID, Actor::Agent).await.is_err());
  assert!(f.sessions.state(ID, Actor::Agent).await.is_err());
  assert!(f
    .sessions
    .request(ID, Actor::Agent, Command::Status {})
    .await
    .is_err());
  assert!(f.sessions.stop(ID, Actor::Agent).await.is_err());
  assert!(matches!(
    f.sessions
      .request(ID, Actor::Person, Command::Status {})
      .await
      .unwrap(),
    Reply::Status { .. }
  ));
  f.manager.set_agent_access(ID, true).unwrap();
  assert_eq!(
    f.sessions.capture(ID, Actor::Agent).await.unwrap().rgba,
    [1, 2, 3, 255]
  );
  f.sessions.stop(ID, Actor::Agent).await.unwrap();
  assert_eq!(f.sessions.state(ID, Actor::Person).await.unwrap(), None);
  assert_eq!(f.trace(), "status\ncapture\nstop\n");
  other.start(ID, &f.helper, f.boot("normal")).await.unwrap();
  other.stop(ID, Actor::Person).await.unwrap();
}

#[tokio::test]
async fn revoked_capture_is_drained_without_returning_guest_pixels() {
  let f = Fixture::new();
  f.start("cancel").await;
  let sessions = f.sessions.clone();
  let task = tokio::spawn(async move { sessions.capture(ID, Actor::Agent).await });
  f.marker("partial").await;
  f.manager.set_agent_access(ID, false).unwrap();
  f.signal("continue");
  assert!(task.await.unwrap().is_err());
  f.sessions
    .request(ID, Actor::Person, Command::Status {})
    .await
    .unwrap();
  f.sessions.stop(ID, Actor::Person).await.unwrap();
  assert_eq!(f.trace(), "capture\nstatus\nstop\n");
}

#[tokio::test]
async fn queued_input_rechecks_policy_after_a_cancelled_frame() {
  let f = Fixture::new();
  f.start("cancel").await;
  let owner = f.sessions.acquire_input(ID, Actor::Agent).await.unwrap();
  let sessions = f.sessions.clone();
  let capture = tokio::spawn(async move { sessions.capture(ID, Actor::Agent).await });
  f.marker("partial").await;
  capture.abort();
  assert!(matches!(capture.await, Err(error) if error.is_cancelled()));
  let pending = tokio::spawn(async move { owner.send(input()).await });
  wait_lock(&f.lock(""), true).await;
  f.manager.set_agent_access(ID, false).unwrap();
  f.signal("continue");
  assert!(pending.await.unwrap().is_err());
  f.sessions.stop(ID, Actor::Person).await.unwrap();
  assert!(!f.trace().lines().any(|line| line == "input"));
}

#[tokio::test]
async fn revocation_after_input_dispatch_releases_held_input() {
  let f = Fixture::new();
  f.start("inputdelay").await;
  let owner = f.sessions.acquire_input(ID, Actor::Agent).await.unwrap();
  let pending = tokio::spawn(async move { owner.send(input()).await });
  f.marker("inputreceived").await;
  f.manager.set_agent_access(ID, false).unwrap();
  f.signal("inputcontinue");
  assert!(pending.await.unwrap().is_err());
  f.sessions.stop(ID, Actor::Person).await.unwrap();
  assert!(f.trace().starts_with("input\nrelease\n"));
  assert!(f.trace().ends_with("stop\n"));
}

#[tokio::test]
async fn cancelled_startup_keeps_ownership_until_cleanup() {
  let f = Fixture::new();
  let sessions = f.sessions.clone();
  let helper = f.helper.clone();
  let boot = f.boot("startupgate");
  let pending = tokio::spawn(async move { sessions.start(ID, &helper, boot).await });
  f.marker("pid").await;
  pending.abort();
  assert!(matches!(pending.await, Err(error) if error.is_cancelled()));
  let other = Sessions::new(f.manager.clone());
  assert!(other.start(ID, &f.helper, f.boot("normal")).await.is_err());
  f.signal("startupcontinue");
  wait_lock(&f.lock(".runtime"), false).await;
  assert_eq!(f.trace(), "stop\n");
  other.start(ID, &f.helper, f.boot("normal")).await.unwrap();
  other.stop(ID, Actor::Person).await.unwrap();
}

#[tokio::test]
async fn registry_teardown_retains_ownership_until_process_exit() {
  let f = Fixture::new();
  f.start("stopdelay").await;
  let mut viewer = f.sessions.watch(ID).await.unwrap();
  let other = Sessions::new(f.manager.clone());
  let lock = f.lock(".runtime");
  let probe = f.probe();
  let helper = f.helper.clone();
  let boot = f.boot("normal");
  let Fixture {
    root,
    manager: _,
    sessions,
    helper: _,
  } = f;
  drop(sessions);
  wait_file(&probe.join("stopreceived")).await;
  assert!(other.start(ID, &helper, boot).await.is_err());
  std::fs::write(probe.join("stopcontinue"), []).unwrap();
  tokio::time::timeout(Duration::from_secs(3), async {
    while !matches!(*viewer.borrow_and_update(), State::Stopped(_)) {
      viewer.changed().await.unwrap();
    }
  })
  .await
  .unwrap();
  assert!(lock.try_lock_exclusive().is_err());
  std::fs::write(probe.join("exitcontinue"), []).unwrap();
  wait_lock(&lock, false).await;
  drop(root);
}

#[tokio::test]
async fn legacy_instances_are_preserved_and_not_started_by_the_native_runtime() {
  let f = Fixture::new();
  let legacy = f.manager.root.join("lima").join(ID);
  std::fs::create_dir_all(&legacy).unwrap();
  std::fs::write(legacy.join("disk"), b"existing guest data").unwrap();
  assert!(f
    .sessions
    .start(ID, &f.helper, f.boot("normal"))
    .await
    .unwrap_err()
    .to_string()
    .contains("migration"));
  assert!(!f.probe().join("pid").exists());
  assert_eq!(
    std::fs::read(legacy.join("disk")).unwrap(),
    b"existing guest data"
  );
}

#[tokio::test]
async fn guest_shutdown_can_restart_in_the_same_registry() {
  let f = Fixture::new();
  f.start("shutdown").await;
  let mut viewer = f.sessions.watch(ID).await.unwrap();
  tokio::time::timeout(Duration::from_secs(3), async {
    while !matches!(*viewer.borrow_and_update(), State::Stopped(_)) {
      viewer.changed().await.unwrap();
    }
  })
  .await
  .unwrap();
  f.start("normal").await;
  assert_eq!(
    f.sessions.state(ID, Actor::Person).await.unwrap(),
    Some(State::Running)
  );
  f.sessions.stop(ID, Actor::Person).await.unwrap();
}

#[tokio::test]
async fn startup_rejection_releases_ownership_before_returning() {
  let f = Fixture::new();
  assert!(f
    .sessions
    .start(ID, &f.helper, f.boot("startupreject"))
    .await
    .is_err());
  assert!(f.lock(".runtime").try_lock_exclusive().is_ok());
  assert_eq!(f.sessions.state(ID, Actor::Person).await.unwrap(), None);
  f.start("normal").await;
  f.sessions.stop(ID, Actor::Person).await.unwrap();
}
