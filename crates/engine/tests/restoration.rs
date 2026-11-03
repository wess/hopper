#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "snapshotcontroller/fixture.rs"]
mod fixture;
use engine::machines::{native::State, Actor};
use fixture::{Fixture, ID};
use model::native::Command;
use std::{fs, time::Duration};
const SOURCE: &str = "02020202-0202-0202-0202-020202020202";

async fn source() -> Fixture {
  let f = Fixture::new("snapshotgates").await;
  f.ready(SOURCE);
  f.marker("pollcontinue");
  f.marker("cancelcontinue");
  f.source
    .sessions
    .snapshot(ID, Actor::Person, SOURCE)
    .await
    .unwrap();
  for name in [
    "pollcontinue",
    "cancelcontinue",
    "snapshotpoll",
    "snapshotcancel",
  ] {
    fs::remove_file(f.variables.join(name)).unwrap();
  }
  f
}

#[tokio::test]
async fn cancelled_restore_drains_prior_capture_and_undoes_only_its_own_pause() {
  for paused in [false, true] {
    let f = source().await;
    if paused {
      f.source
        .sessions
        .request(ID, Actor::Person, Command::Pause {})
        .await
        .unwrap();
    }
    let before = f.trace();
    let sessions = f.source.sessions.clone();
    let task =
      tokio::spawn(async move { sessions.restore_snapshot(ID, Actor::Agent, SOURCE).await });
    f.wait("snapshotpoll").await;
    assert!(f.locked());
    task.abort();
    assert!(task.await.err().unwrap().is_cancelled());
    f.marker("pollcontinue");
    f.wait("snapshotcancel").await;
    assert!(f.locked());
    assert!(!f.trace()[before.len()..].contains("stop"));
    f.marker("cancelcontinue");
    f.released().await;
    assert_eq!(
      f.source.sessions.state(ID, Actor::Person).await.unwrap(),
      Some(if paused {
        State::Paused
      } else {
        State::Running
      })
    );
    assert_eq!(fs::read_dir(f.root.join("snapshots")).unwrap().count(), 1);
    assert!(!f.root.join("restore.hrr").exists());
    f.source.sessions.stop(ID, Actor::Person).await.unwrap();
  }
}

#[tokio::test]
async fn revoked_restore_cleans_up_without_resuming_or_reauthorizing_the_source() {
  let f = source().await;
  let sessions = f.source.sessions.clone();
  let task = tokio::spawn(async move {
    sessions
      .restore_snapshot_with_generation(ID, SOURCE, 0)
      .await
  });
  f.wait("snapshotpoll").await;
  f.source.manager.set_agent_access(ID, false).unwrap();
  f.source.manager.set_agent_access(ID, true).unwrap();
  f.marker("pollcontinue");
  f.wait("snapshotcancel").await;
  assert!(f.locked());
  f.marker("cancelcontinue");
  assert!(task.await.unwrap().is_err());
  f.released().await;
  assert_eq!(
    f.source.sessions.state(ID, Actor::Person).await.unwrap(),
    Some(State::Paused)
  );
  assert!(!f.trace().contains("resume") && !f.trace().contains("stop"));
  assert!(!f.root.join("restore.hrr").exists());
  f.source.sessions.stop(ID, Actor::Person).await.unwrap();
}

#[tokio::test]
async fn missing_source_and_stale_original_generation_fail_before_pausing() {
  let f = Fixture::new("snapshotgates").await;
  assert!(f
    .source
    .sessions
    .restore_snapshot(ID, Actor::Person, SOURCE)
    .await
    .is_err());
  assert!(f.trace().is_empty());
  f.source.manager.set_agent_access(ID, false).unwrap();
  f.source.manager.set_agent_access(ID, true).unwrap();
  assert!(f
    .source
    .sessions
    .restore_snapshot_with_generation(ID, SOURCE, 0)
    .await
    .is_err());
  assert!(f.trace().is_empty());
  assert_eq!(
    f.source.sessions.state(ID, Actor::Person).await.unwrap(),
    Some(State::Running)
  );
  f.source.sessions.stop(ID, Actor::Person).await.unwrap();
}

#[tokio::test]
async fn pending_or_incompatible_predecessor_rejects_before_capture_or_pause() {
  use engine::machines::native::restore::{Phase, Record};
  use std::os::unix::fs::PermissionsExt;
  for phase in [Phase::Prepared, Phase::Attempted, Phase::Active] {
    let f = source().await;
    let record = Record {
      machine: model::native::machine_identity(ID).unwrap(),
      generation: [4; 16],
      snapshot: [2; 16],
      configuration: if phase == Phase::Active {
        [8; 32]
      } else {
        [7; 32]
      },
      rollback: [5; 16],
      previous: None,
      phase,
    }
    .encode()
    .unwrap();
    let path = f.root.join("restore.hrr");
    fs::write(&path, &record).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
    let before = f.trace();
    assert!(f
      .source
      .sessions
      .restore_snapshot(ID, Actor::Agent, SOURCE)
      .await
      .is_err());
    assert_eq!(f.trace(), before);
    assert_eq!(fs::read(path).unwrap(), record);
    f.source.sessions.stop(ID, Actor::Person).await.unwrap();
  }
}

#[tokio::test]
async fn cancelled_repeated_restore_preserves_active_predecessor_and_source() {
  use engine::machines::native::restore::{Phase, Record};
  use std::os::unix::fs::PermissionsExt;
  let f = source().await;
  let record = Record {
    machine: model::native::machine_identity(ID).unwrap(),
    generation: [4; 16],
    snapshot: [2; 16],
    configuration: [7; 32],
    rollback: [5; 16],
    previous: None,
    phase: Phase::Active,
  }
  .encode()
  .unwrap();
  let path = f.root.join("restore.hrr");
  fs::write(&path, &record).unwrap();
  fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
  // Metadata-only predecessor fixture: cancellation must never reach generation admission.
  let sessions = f.source.sessions.clone();
  let task = tokio::spawn(async move { sessions.restore_snapshot(ID, Actor::Agent, SOURCE).await });
  f.wait("snapshotpoll").await;
  task.abort();
  assert!(task.await.err().unwrap().is_cancelled());
  f.marker("pollcontinue");
  f.wait("snapshotcancel").await;
  f.marker("cancelcontinue");
  f.released().await;
  assert_eq!(fs::read(path).unwrap(), record);
  assert_eq!(
    f.source.sessions.state(ID, Actor::Person).await.unwrap(),
    Some(State::Running)
  );
  assert!(!f.trace().contains("stop"));
  assert_eq!(fs::read_dir(f.root.join("snapshots")).unwrap().count(), 1);
  f.source.sessions.stop(ID, Actor::Person).await.unwrap();
}

#[tokio::test]
async fn malformed_recovery_intent_is_preserved_without_capturing_or_stopping() {
  let f = source().await;
  let receipt = f.root.join("restore.hrr");
  fs::write(&receipt, b"preserve invalid recovery intent").unwrap();
  use std::os::unix::fs::PermissionsExt;
  fs::set_permissions(&receipt, fs::Permissions::from_mode(0o400)).unwrap();
  let before = f.trace();
  assert!(f
    .source
    .sessions
    .restore_snapshot(ID, Actor::Person, SOURCE)
    .await
    .is_err());
  assert_eq!(f.trace(), before);
  assert_eq!(
    fs::read(&receipt).unwrap(),
    b"preserve invalid recovery intent"
  );
  assert_eq!(
    f.source.sessions.state(ID, Actor::Person).await.unwrap(),
    Some(State::Running)
  );
  // leave no live process when the isolated fixture is removed.
  tokio::time::timeout(
    Duration::from_secs(2),
    f.source.sessions.stop(ID, Actor::Person),
  )
  .await
  .unwrap()
  .unwrap();
}

#[tokio::test]
async fn invalid_hardware_after_prior_publication_preserves_checkpoint_and_resumes_source() {
  for paused in [false, true] {
    let f = source().await;
    if paused {
      f.source
        .sessions
        .request(ID, Actor::Person, Command::Pause {})
        .await
        .unwrap();
    }
    let before = f.trace();
    let sessions = f.source.sessions.clone();
    let task =
      tokio::spawn(async move { sessions.restore_snapshot(ID, Actor::Agent, SOURCE).await });
    f.wait("snapshotpoll").await;
    let prior = fs::read_to_string(f.variables.join("snapshotidentity")).unwrap();
    assert_ne!(prior, SOURCE);
    f.ready(&prior);
    f.marker("pollcontinue");
    f.marker("cancelcontinue");
    let error = task.await.unwrap().err().unwrap();
    assert!(format!("{error:#}").contains("Restore source validation failed before stop"));
    f.released().await;
    assert!(!f.trace().contains("stop"));
    assert_eq!(
      f.source.sessions.state(ID, Actor::Person).await.unwrap(),
      Some(if paused {
        State::Paused
      } else {
        State::Running
      })
    );
    if paused {
      assert!(!f.trace()[before.len()..].contains("resume"));
    }
    let entries = engine::machines::native::snapshot::Policy::person(f.source.manager.clone(), ID)
      .unwrap()
      .inventory()
      .unwrap();
    assert!(entries
      .iter()
      .any(|entry| entry.id == prior && entry.name.as_deref() == Some("Before restore")));
    assert!(!f.root.join("restore.hrr").exists());
    assert!(!f.root.join("generations").exists());
    f.source.sessions.stop(ID, Actor::Person).await.unwrap();
  }
}
