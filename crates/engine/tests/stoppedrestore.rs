#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[allow(dead_code)]
#[path = "snapshotcontroller/fixture.rs"]
mod fixture;
use engine::machines::{
  native::restore::{Owner, Phase, Record},
  Actor,
};
use fixture::{Fixture, ID};
use model::native::Boot;
use std::{fs, os::unix::fs::PermissionsExt, time::Duration};
const SOURCE: &str = "02020202-0202-0202-0202-020202020202";

async fn source(mode: &str) -> (Fixture, Boot, Vec<u8>) {
  let f = Fixture::new("snapshotgates").await;
  f.ready(SOURCE);
  f.marker("pollcontinue");
  f.marker("cancelcontinue");
  f.source
    .sessions
    .snapshot(ID, Actor::Person, SOURCE)
    .await
    .unwrap();
  f.source.sessions.stop(ID, Actor::Person).await.unwrap();
  for name in [
    "pid",
    "pollcontinue",
    "cancelcontinue",
    "snapshotpoll",
    "snapshotcancel",
    "snapshotreceipt",
    "trace",
    "stopacknowledged",
  ] {
    let path = f.variables.join(name);
    if path.exists() {
      fs::remove_file(path).unwrap();
    }
  }
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
  fs::write(f.root.join("restore.hrr"), &record).unwrap();
  fs::set_permissions(
    f.root.join("restore.hrr"),
    fs::Permissions::from_mode(0o400),
  )
  .unwrap();
  fs::write(
    f.variables.join("configuration"),
    serde_json::to_vec(&vec![7; 32]).unwrap(),
  )
  .unwrap();
  let mut boot = f.source.boot(mode);
  boot.store = f.variables.to_string_lossy().into();
  (f, boot, record)
}

async fn released(f: &Fixture) {
  tokio::time::timeout(Duration::from_secs(4), async {
    while Owner::person(f.source.manager.clone(), ID).is_err() {
      tokio::time::sleep(Duration::from_millis(5)).await;
    }
  })
  .await
  .unwrap();
}

#[tokio::test]
async fn stopped_facade_rejects_stale_policy_without_touching_selection_or_worker() {
  let (f, _, record) = source("restored").await;
  f.source.manager.set_agent_access(ID, false).unwrap();
  f.source.manager.set_agent_access(ID, true).unwrap();
  assert!(f
    .source
    .sessions
    .restore_snapshot_with_generation(ID, SOURCE, 0)
    .await
    .is_err());
  released(&f).await;
  assert_eq!(fs::read(f.root.join("restore.hrr")).unwrap(), record);
  assert!(!f.variables.join("pid").exists());
  assert!(f.trace().is_empty());
}

#[tokio::test]
async fn stopped_facade_requires_exclusive_runtime_before_asset_lookup() {
  let (f, _, record) = source("restored").await;
  let file = fs::OpenOptions::new()
    .read(true)
    .write(true)
    .open(
      f.source
        .manager
        .root
        .join("locks")
        .join(format!("{ID}.runtime")),
    )
    .unwrap();
  let runtime = store::lock::exclusive(file).unwrap();
  let error = f
    .source
    .sessions
    .restore_snapshot(ID, Actor::Agent, SOURCE)
    .await
    .err()
    .unwrap();
  assert!(
    format!("{error:#}").contains("Another operation is in progress"),
    "{error:#}"
  );
  assert_eq!(fs::read(f.root.join("restore.hrr")).unwrap(), record);
  assert!(!f.variables.join("pid").exists());
  assert!(f.trace().is_empty());
  drop(runtime);
  released(&f).await;
}

#[tokio::test]
async fn cancelled_held_start_reaps_without_capturing_or_replacing_intent() {
  let (f, boot, record) = source("generationstartupgate").await;
  let owned = Owner::agent(f.source.manager.clone(), ID, 0).unwrap();
  let sessions = f.source.sessions.clone();
  let helper = f.source.helper.clone();
  let task =
    tokio::spawn(async move { sessions.restore_stopped(&helper, boot, SOURCE, owned).await });
  f.wait("pid").await;
  task.abort();
  assert!(task.await.err().unwrap().is_cancelled());
  released(&f).await;
  let pid = fs::read_to_string(f.variables.join("pid"))
    .unwrap()
    .parse()
    .unwrap();
  assert_ne!(unsafe { libc::kill(pid, 0) }, 0);
  assert_eq!(fs::read(f.root.join("restore.hrr")).unwrap(), record);
  assert!(!f.trace().contains("snapshotStart") && !f.trace().contains("restoreResume"));
  assert!(f
    .source
    .sessions
    .state(ID, Actor::Person)
    .await
    .unwrap()
    .is_none());
}

#[tokio::test]
async fn revoked_held_capture_does_not_resume_or_reauthorize_after_off_on() {
  let (f, boot, record) = source("restored").await;
  let owned = Owner::agent(f.source.manager.clone(), ID, 0).unwrap();
  let sessions = f.source.sessions.clone();
  let helper = f.source.helper.clone();
  let task =
    tokio::spawn(async move { sessions.restore_stopped(&helper, boot, SOURCE, owned).await });
  f.wait("snapshotpoll").await;
  f.source.manager.set_agent_access(ID, false).unwrap();
  f.source.manager.set_agent_access(ID, true).unwrap();
  assert!(task.await.unwrap().is_err());
  released(&f).await;
  assert_eq!(fs::read(f.root.join("restore.hrr")).unwrap(), record);
  assert!(!f.trace().contains("resume") && !f.trace().contains("restoreResume"));
  assert_eq!(fs::read_dir(f.root.join("snapshots")).unwrap().count(), 1);
}

#[tokio::test]
async fn cancelled_held_capture_retains_exclusion_through_stop_ack_and_actual_exit() {
  let (f, boot, record) = source("restored").await;
  f.marker("stopgate");
  let owned = Owner::agent(f.source.manager.clone(), ID, 0).unwrap();
  let sessions = f.source.sessions.clone();
  let helper = f.source.helper.clone();
  let task =
    tokio::spawn(async move { sessions.restore_stopped(&helper, boot, SOURCE, owned).await });
  f.wait("snapshotpoll").await;
  task.abort();
  assert!(task.await.err().unwrap().is_cancelled());
  f.wait("stopreceived").await;
  assert!(Owner::person(f.source.manager.clone(), ID).is_err());
  f.marker("stopcontinue");
  f.wait("stopacknowledged").await;
  assert!(Owner::person(f.source.manager.clone(), ID).is_err());
  f.marker("exitcontinue");
  released(&f).await;
  assert_eq!(fs::read(f.root.join("restore.hrr")).unwrap(), record);
  assert!(f
    .source
    .sessions
    .state(ID, Actor::Person)
    .await
    .unwrap()
    .is_none());
}
