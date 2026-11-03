#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "snapshotcontroller/fixture.rs"]
mod fixture;
use engine::machines::{
  native::{sessions::remote::restoration, State},
  Actor,
};
use fixture::{Fixture, ID};
use serde_json::{json, Value};
use std::fs;
use tokio::{
  io::{AsyncReadExt, AsyncWriteExt},
  net::UnixStream,
};
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
    "trace",
  ] {
    fs::remove_file(f.variables.join(name)).unwrap();
  }
  f
}

async fn connect(f: &Fixture, generation: Option<u64>, snapshot: &str) -> UnixStream {
  let mut stream = UnixStream::connect(f.source.manager.root.join("agents/native.sock"))
    .await
    .unwrap();
  let header = serde_json::to_vec(&json!({"vmId":ID,"agentGeneration":generation,
    "operation":{"restore":{"id":snapshot}}}))
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
    std::time::Duration::from_secs(3),
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
async fn missing_stale_policy_and_invalid_snapshot_reject_before_dispatch() {
  let f = source().await;
  let _server = f.source.sessions.serve_agents().unwrap();
  for (generation, snapshot) in [(None, SOURCE), (Some(1), SOURCE), (Some(0), "../foreign")] {
    let mut stream = connect(&f, generation, snapshot).await;
    assert_eq!(reply(&mut stream).await["type"], "error");
    assert!(f.trace().is_empty());
  }
  assert!(restoration::restore(&f.source.manager, ID, 0, "../foreign")
    .await
    .is_err());
  assert!(f.trace().is_empty());
  f.source.sessions.stop(ID, Actor::Person).await.unwrap();
}

#[tokio::test]
async fn disconnect_and_pipelining_cancel_prior_capture_and_preserve_running_source() {
  for pipeline in [false, true] {
    let f = source().await;
    let _server = f.source.sessions.serve_agents().unwrap();
    let mut stream = connect(&f, Some(0), SOURCE).await;
    f.wait("snapshotpoll").await;
    if pipeline {
      stream.write_all(b"x").await.unwrap();
    }
    drop(stream);
    f.marker("pollcontinue");
    f.wait("snapshotcancel").await;
    assert!(f.locked());
    assert!(!f.trace().contains("stop"));
    f.marker("cancelcontinue");
    f.released().await;
    assert_eq!(
      f.source.sessions.state(ID, Actor::Person).await.unwrap(),
      Some(State::Running)
    );
    assert!(!f.root.join("restore.hrr").exists());
    assert_eq!(fs::read_dir(f.root.join("snapshots")).unwrap().count(), 1);
    f.source.sessions.stop(ID, Actor::Person).await.unwrap();
  }
}

#[tokio::test]
async fn off_on_revocation_cleans_prior_without_resuming_or_reauthorizing() {
  let f = source().await;
  let _server = f.source.sessions.serve_agents().unwrap();
  let mut stream = connect(&f, Some(0), SOURCE).await;
  f.wait("snapshotpoll").await;
  f.source.manager.set_agent_access(ID, false).unwrap();
  f.source.manager.set_agent_access(ID, true).unwrap();
  f.marker("pollcontinue");
  f.wait("snapshotcancel").await;
  assert!(f.locked());
  f.marker("cancelcontinue");
  assert_eq!(reply(&mut stream).await["type"], "error");
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
async fn restore_protocol_rejects_boot_paths_before_worker_dispatch() {
  let f = source().await;
  let _server = f.source.sessions.serve_agents().unwrap();
  let mut stream = UnixStream::connect(f.source.manager.root.join("agents/native.sock"))
    .await
    .unwrap();
  let header = serde_json::to_vec(&json!({"vmId":ID,"agentGeneration":0,
    "operation":{"restore":{"id":SOURCE,"boot":{"disk":"/tmp/foreign"}}}}))
  .unwrap();
  stream.write_all(b"HPA1").await.unwrap();
  stream
    .write_all(&(header.len() as u32).to_le_bytes())
    .await
    .unwrap();
  stream.write_all(&header).await.unwrap();
  let mut byte = [0; 1];
  let closed = tokio::time::timeout(std::time::Duration::from_secs(2), stream.read(&mut byte))
    .await
    .unwrap();
  assert!(matches!(closed, Ok(0) | Err(_)));
  assert!(f.trace().is_empty());
  assert!(!f.root.join("restore.hrr").exists());
  f.source.sessions.stop(ID, Actor::Person).await.unwrap();
}
