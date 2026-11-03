#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "snapshotcontroller/fixture.rs"]
mod fixture;
use engine::machines::{native::State, Actor};
use fixture::{Fixture, ID};
use std::{
  future::{poll_fn, Future},
  task::Poll,
  time::Duration,
};
const SNAPSHOT: &str = "02020202-0202-0202-0202-020202020202";

#[tokio::test]
async fn caller_cancellation_retains_ownership_through_drain_and_cleanup() {
  let f = Fixture::new("snapshotgates").await;
  let sessions = f.source.sessions.clone();
  let caller = tokio::spawn(async move { sessions.snapshot(ID, Actor::Agent, SNAPSHOT).await });
  f.wait("snapshotpoll").await;
  let mut queued = Box::pin(f.source.sessions.snapshot(
    ID,
    Actor::Agent,
    "03030303-0303-0303-0303-030303030303",
  ));
  poll_fn(|cx| {
    assert!(queued.as_mut().poll(cx).is_pending());
    Poll::Ready(())
  })
  .await;
  caller.abort();
  assert!(caller.await.unwrap_err().is_cancelled());
  assert!(f.locked());
  f.source.manager.set_agent_access(ID, false).unwrap();
  f.source.manager.set_agent_access(ID, true).unwrap();
  f.marker("pollcontinue");
  f.wait("snapshotcancel").await;
  assert!(f.locked());
  assert!(!f.trace().contains("stop"));
  assert!(tokio::time::timeout(
    Duration::from_millis(20),
    f.source.sessions.stop(ID, Actor::Person)
  )
  .await
  .is_err());

  f.marker("cancelcontinue");
  assert!(queued.await.is_err());
  f.released().await;
  assert_eq!(f.trace().matches("snapshotStart").count(), 1);
  assert!(!f
    .root
    .join("snapshots")
    .join(SNAPSHOT.replace('-', ""))
    .exists());
  assert_eq!(
    f.source.sessions.state(ID, Actor::Person).await.unwrap(),
    Some(State::Running)
  );
  f.source.sessions.stop(ID, Actor::Person).await.unwrap();
}

#[tokio::test]
async fn completed_publication_survives_cancellation_during_cleanup() {
  let f = Fixture::new("snapshotgates").await;
  f.ready(SNAPSHOT);
  f.marker("pollcontinue");
  let sessions = f.source.sessions.clone();
  let caller = tokio::spawn(async move {
    sessions
      .snapshot_named_with_generation(ID, SNAPSHOT, "Before updates", 0)
      .await
  });
  f.wait("snapshotcancel").await;
  let manifest = f
    .root
    .join("snapshots")
    .join(SNAPSHOT.replace('-', ""))
    .join("manifest.hsm");
  let saved = std::fs::read(&manifest).unwrap();
  caller.abort();
  assert!(caller.await.unwrap_err().is_cancelled());
  assert!(f.locked());
  f.marker("cancelcontinue");
  f.released().await;
  assert_eq!(std::fs::read(manifest).unwrap(), saved);
  let entries = engine::machines::native::snapshot::Policy::person(f.source.manager.clone(), ID)
    .unwrap()
    .inventory()
    .unwrap();
  assert_eq!(entries[0].name.as_deref(), Some("Before updates"));
  assert!(entries[0].created_unix_seconds.unwrap() > 0);
  f.source.sessions.stop(ID, Actor::Person).await.unwrap();
}

#[tokio::test]
async fn revoked_original_policy_is_checked_before_commit_but_not_cleanup() {
  let f = Fixture::new("snapshotgates").await;
  f.ready(SNAPSHOT);
  let sessions = f.source.sessions.clone();
  let caller = tokio::spawn(async move { sessions.snapshot(ID, Actor::Agent, SNAPSHOT).await });
  f.wait("snapshotpoll").await;
  f.source.manager.set_agent_access(ID, false).unwrap();
  f.source.manager.set_agent_access(ID, true).unwrap();
  f.marker("pollcontinue");
  f.wait("snapshotcancel").await;
  assert!(f.locked());
  assert!(!f
    .root
    .join("snapshots")
    .join(SNAPSHOT.replace('-', ""))
    .exists());
  f.marker("cancelcontinue");
  assert!(caller.await.unwrap().is_err());
  f.released().await;
  f.source.sessions.stop(ID, Actor::Person).await.unwrap();
}

#[tokio::test]
async fn rejected_capture_cleans_up_without_stopping_source() {
  let f = Fixture::new("snapshotstartreject").await;
  assert!(f
    .source
    .sessions
    .snapshot(ID, Actor::Agent, SNAPSHOT)
    .await
    .is_err());
  assert_eq!(f.trace(), "snapshotStart\nsnapshotCancel\n");
  f.released().await;
  assert_eq!(
    f.source.sessions.state(ID, Actor::Person).await.unwrap(),
    Some(State::Running)
  );
  f.source.sessions.stop(ID, Actor::Person).await.unwrap();
}

#[tokio::test]
async fn unacknowledged_cleanup_stops_and_reaps_worker_before_release() {
  let f = Fixture::new("snapshotcancelreject").await;
  assert!(f
    .source
    .sessions
    .snapshot(ID, Actor::Agent, SNAPSHOT)
    .await
    .is_err());
  f.released().await;
  assert!(f.trace().ends_with("snapshotCancel\nstop\n"));
  assert!(matches!(
    f.source.sessions.state(ID, Actor::Person).await.unwrap(),
    Some(State::Stopped(_))
  ));
  f.source.sessions.stop(ID, Actor::Person).await.unwrap();
}

#[tokio::test]
async fn startup_namespace_replacement_rejects_before_worker_dispatch() {
  use std::os::unix::fs::PermissionsExt;
  let f = Fixture::new("normal").await;
  let old = f.root.with_extension("preserved");
  std::fs::rename(&f.root, &old).unwrap();
  std::fs::create_dir(&f.root).unwrap();
  std::fs::set_permissions(&f.root, std::fs::Permissions::from_mode(0o700)).unwrap();
  std::fs::create_dir(&f.variables).unwrap();
  assert!(f
    .source
    .sessions
    .snapshot(ID, Actor::Agent, SNAPSHOT)
    .await
    .is_err());
  assert!(!f.root.join("snapshots").exists());
  assert!(!f.trace().contains("snapshot"));
  f.source.sessions.stop(ID, Actor::Person).await.unwrap();
}

#[tokio::test]
async fn upstream_generation_cannot_refresh_after_an_off_on_toggle() {
  let f = Fixture::new("normal").await;
  f.source.manager.set_agent_access(ID, false).unwrap();
  f.source.manager.set_agent_access(ID, true).unwrap();
  assert!(f
    .source
    .sessions
    .snapshot_with_generation(ID, SNAPSHOT, 0)
    .await
    .is_err());
  assert_eq!(f.trace(), "");
  f.source.sessions.stop(ID, Actor::Person).await.unwrap();
}

#[tokio::test]
async fn source_shutdown_prevents_publication_and_retains_staged_data() {
  let f = Fixture::new("snapshotsourceshutdown").await;
  f.ready(SNAPSHOT);
  assert!(f
    .source
    .sessions
    .snapshot(ID, Actor::Agent, SNAPSHOT)
    .await
    .is_err());
  f.released().await;
  assert!(!f
    .root
    .join("snapshots")
    .join(SNAPSHOT.replace('-', ""))
    .exists());
  assert_eq!(
    std::fs::read_dir(f.root.join("snapshots")).unwrap().count(),
    1
  );
  assert!(matches!(
    f.source.sessions.state(ID, Actor::Person).await.unwrap(),
    Some(State::Stopped(_))
  ));
  f.source.sessions.stop(ID, Actor::Person).await.unwrap();
}
