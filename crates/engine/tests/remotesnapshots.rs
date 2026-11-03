#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "snapshotcontroller/fixture.rs"]
mod fixture;
use engine::machines::{
  native::{sessions::remote::snapshots, snapshot::Policy},
  Actor,
};
use fixture::{Fixture, ID};
use serde_json::{json, Value};
use tokio::{
  io::{AsyncReadExt, AsyncWriteExt},
  net::UnixStream,
};
const SNAPSHOT: &str = "02020202-0202-0202-0202-020202020202";

async fn connect(f: &Fixture, generation: Option<u64>, name: &str) -> UnixStream {
  let mut stream = UnixStream::connect(f.source.manager.root.join("agents/native.sock"))
    .await
    .unwrap();
  let header = serde_json::to_vec(&json!({"vmId":ID,"agentGeneration":generation,
    "operation":{"snapshot":{"id":SNAPSHOT,"name":name}}}))
  .unwrap();
  stream.write_all(b"HPA1").await.unwrap();
  stream
    .write_all(&(header.len() as u32).to_le_bytes())
    .await
    .unwrap();
  stream.write_all(&header).await.unwrap();
  stream
}

async fn reply(stream: &mut UnixStream) -> Value {
  let mut prefix = [0; 8];
  tokio::time::timeout(
    std::time::Duration::from_secs(2),
    stream.read_exact(&mut prefix),
  )
  .await
  .unwrap()
  .unwrap();
  assert_eq!(&prefix[..4], b"HPA1");
  let length = u32::from_le_bytes(prefix[4..].try_into().unwrap()) as usize;
  assert!(length <= 4096);
  let mut bytes = vec![0; length];
  stream.read_exact(&mut bytes).await.unwrap();
  serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn socket_captures_named_package_through_original_owned_controller() {
  let f = Fixture::new("normal").await;
  f.ready(SNAPSHOT);
  let _server = f.source.sessions.serve_agents().unwrap();
  snapshots::capture(&f.source.manager, ID, 0, SNAPSHOT, "Before updates")
    .await
    .unwrap();
  let entries = Policy::agent(f.source.manager.clone(), ID, 0)
    .unwrap()
    .inventory()
    .unwrap();
  assert_eq!(entries[0].name.as_deref(), Some("Before updates"));
  assert_eq!(entries[0].id, SNAPSHOT);
  assert_eq!(f.trace(), "snapshotStart\nsnapshotPoll\nsnapshotCancel\n");
  assert!(!f.locked());
  f.source.sessions.stop(ID, Actor::Person).await.unwrap();
}

#[tokio::test]
async fn disconnect_or_pipelining_retains_cleanup_ownership_without_publication() {
  for pipeline in [false, true] {
    let f = Fixture::new("snapshotgates").await;
    let _server = f.source.sessions.serve_agents().unwrap();
    let mut stream = connect(&f, Some(0), "Before updates").await;
    f.wait("snapshotpoll").await;
    if pipeline {
      stream.write_all(b"x").await.unwrap();
    }
    drop(stream);
    f.marker("pollcontinue");
    f.wait("snapshotcancel").await;
    assert!(f.locked());
    assert!(!f
      .root
      .join("snapshots")
      .join(SNAPSHOT.replace('-', ""))
      .exists());
    f.marker("cancelcontinue");
    f.released().await;
    f.source.sessions.stop(ID, Actor::Person).await.unwrap();
  }
}

#[tokio::test]
async fn missing_stale_policy_and_invalid_names_reject_before_worker_dispatch() {
  let f = Fixture::new("normal").await;
  let _server = f.source.sessions.serve_agents().unwrap();
  for (generation, name) in [
    (None, "Valid"),
    (Some(1), "Valid"),
    (Some(0), " "),
    (Some(0), "line\nbreak"),
  ] {
    let mut stream = connect(&f, generation, name).await;
    let response = reply(&mut stream).await;
    assert_eq!(response["type"], "error", "{response}");
    assert_eq!(f.trace(), "");
  }
  assert!(
    snapshots::capture(&f.source.manager, ID, 0, "../foreign", "Valid")
      .await
      .is_err()
  );
  assert_eq!(f.trace(), "");
  f.source.sessions.stop(ID, Actor::Person).await.unwrap();
}

#[tokio::test]
async fn original_policy_off_on_rejects_ready_publication_but_still_cleans_worker() {
  let f = Fixture::new("snapshotgates").await;
  f.ready(SNAPSHOT);
  let _server = f.source.sessions.serve_agents().unwrap();
  let mut stream = connect(&f, Some(0), "Before updates").await;
  f.wait("snapshotpoll").await;
  f.source.manager.set_agent_access(ID, false).unwrap();
  f.source.manager.set_agent_access(ID, true).unwrap();
  f.marker("pollcontinue");
  f.wait("snapshotcancel").await;
  assert!(f.locked());
  f.marker("cancelcontinue");
  assert_eq!(reply(&mut stream).await["type"], "error");
  f.released().await;
  assert!(Policy::person(f.source.manager.clone(), ID)
    .unwrap()
    .inventory()
    .unwrap()
    .is_empty());
  f.source.sessions.stop(ID, Actor::Person).await.unwrap();
}

#[tokio::test]
async fn disconnect_after_publication_preserves_named_capture_until_cleanup_finishes() {
  let f = Fixture::new("snapshotgates").await;
  f.ready(SNAPSHOT);
  f.marker("pollcontinue");
  let _server = f.source.sessions.serve_agents().unwrap();
  let stream = connect(&f, Some(0), "Before updates").await;
  f.wait("snapshotcancel").await;
  let policy = Policy::person(f.source.manager.clone(), ID).unwrap();
  assert_eq!(
    policy.inventory().unwrap()[0].name.as_deref(),
    Some("Before updates")
  );
  drop(stream);
  assert!(f.locked());
  f.marker("cancelcontinue");
  f.released().await;
  assert_eq!(
    policy.inventory().unwrap()[0].name.as_deref(),
    Some("Before updates")
  );
  f.source.sessions.stop(ID, Actor::Person).await.unwrap();
}
