#![cfg(unix)]

#[allow(dead_code)]
#[path = "sessions/fixture.rs"]
mod fixture;

use engine::machines::{native::sessions::remote::control, Actor};
use fixture::{Fixture, ID};
use model::native::{InputDevice, InputEvent};
use std::time::Duration;
use tokio::{
  io::{AsyncReadExt, AsyncWriteExt},
  net::UnixStream,
};

async fn raw(f: &Fixture) -> UnixStream {
  let mut stream = UnixStream::connect(f.manager.root.join("agents/native.sock"))
    .await
    .unwrap();
  write_message(
    &mut stream,
    serde_json::json!({"vmId": ID, "operation": "control"}),
  )
  .await;
  let mut prefix = [0; 8];
  stream.read_exact(&mut prefix).await.unwrap();
  assert_eq!(&prefix[..4], b"HPA1");
  let mut reply = vec![0; u32::from_le_bytes(prefix[4..].try_into().unwrap()) as usize];
  stream.read_exact(&mut reply).await.unwrap();
  assert_eq!(
    serde_json::from_slice::<serde_json::Value>(&reply).unwrap(),
    serde_json::json!({"type": "owned"})
  );
  stream
}

async fn write_message(stream: &mut UnixStream, value: serde_json::Value) {
  let value = serde_json::to_vec(&value).unwrap();
  stream.write_all(b"HPA1").await.unwrap();
  stream
    .write_all(&(value.len() as u32).to_le_bytes())
    .await
    .unwrap();
  stream.write_all(&value).await.unwrap();
}

#[tokio::test]
async fn malformed_control_packets_release_ownership_without_dispatch() {
  let f = Fixture::new();
  f.start("normal").await;
  let _server = f.sessions.serve_agents().unwrap();
  for message in [
    serde_json::json!({"type":"input", "device":"keyboard", "events":[], "path":"/tmp/untrusted"}),
    serde_json::json!({"type":"start", "worker":"/tmp/untrusted"}),
  ] {
    let mut stream = raw(&f).await;
    write_message(&mut stream, message).await;
    let mut bytes = Vec::new();
    tokio::time::timeout(Duration::from_secs(3), stream.read_to_end(&mut bytes))
      .await
      .unwrap()
      .unwrap();
    let next = connect(&f).await;
    next.close().await.unwrap();
  }
  let mut stream = raw(&f).await;
  stream.write_all(b"HPA1").await.unwrap();
  stream.write_all(&4097u32.to_le_bytes()).await.unwrap();
  let mut bytes = Vec::new();
  tokio::time::timeout(Duration::from_secs(3), stream.read_to_end(&mut bytes))
    .await
    .unwrap()
    .unwrap();
  let next = connect(&f).await;
  next.close().await.unwrap();
  assert!(!f.trace().lines().any(|line| line == "input"));
  f.sessions.stop(ID, Actor::Person).await.unwrap();
}

fn events() -> Vec<InputEvent> {
  vec![
    InputEvent {
      kind: 1,
      code: 30,
      value: 1,
    },
    InputEvent {
      kind: 0,
      code: 0,
      value: 0,
    },
  ]
}

async fn connect(f: &Fixture) -> control::Control {
  tokio::time::timeout(Duration::from_secs(3), async {
    loop {
      if let Ok(control) = control::connect(&f.manager, ID).await {
        return control;
      }
      tokio::time::sleep(Duration::from_millis(5)).await;
    }
  })
  .await
  .unwrap()
}

#[tokio::test]
async fn remote_connections_share_input_ownership_with_the_viewer() {
  let f = Fixture::new();
  f.start("normal").await;
  let _server = f.sessions.serve_agents().unwrap();
  let person = f.sessions.acquire_input(ID, Actor::Person).await.unwrap();
  let error = control::connect(&f.manager, ID).await.err().unwrap();
  assert!(error
    .to_string()
    .contains("controlled by another connection"));
  person.close().await.unwrap();
  let mut agent = control::connect(&f.manager, ID).await.unwrap();
  assert!(f.sessions.acquire_input(ID, Actor::Person).await.is_err());
  assert!(control::connect(&f.manager, ID).await.is_err());
  agent.send(InputDevice::Keyboard, events()).await.unwrap();
  agent.close().await.unwrap();
  let person = f.sessions.acquire_input(ID, Actor::Person).await.unwrap();
  person.close().await.unwrap();
  assert_eq!(f.trace(), "release\ninput\nrelease\nrelease\n");
  f.sessions.stop(ID, Actor::Person).await.unwrap();
}

#[tokio::test]
async fn cancellation_closes_the_connection_and_holds_ownership_through_cleanup() {
  let f = Fixture::new();
  f.start("inputdelay").await;
  let _server = f.sessions.serve_agents().unwrap();
  let mut agent = control::connect(&f.manager, ID).await.unwrap();
  let pending = tokio::spawn(async move { agent.send(InputDevice::Keyboard, events()).await });
  f.marker("inputreceived").await;
  pending.abort();
  let _ = pending.await;
  assert!(f.sessions.acquire_input(ID, Actor::Person).await.is_err());
  f.signal("inputcontinue");
  let mut next = connect(&f).await;
  next.send(InputDevice::Keyboard, events()).await.unwrap();
  let trace = f.trace();
  assert!(trace.starts_with("input\nrelease\n"));
  assert!(trace.ends_with("input\n"));
  tokio::time::sleep(Duration::from_millis(30)).await;
  assert_eq!(f.trace(), trace);
  next.close().await.unwrap();
  f.sessions.stop(ID, Actor::Person).await.unwrap();
}

#[tokio::test]
async fn rapid_access_toggle_cannot_revive_an_existing_connection() {
  let f = Fixture::new();
  f.start("normal").await;
  let _server = f.sessions.serve_agents().unwrap();
  let mut old = control::connect(&f.manager, ID).await.unwrap();
  old.send(InputDevice::Keyboard, events()).await.unwrap();
  let initial = f
    .manager
    .machine(ID, Actor::Person)
    .unwrap()
    .agent_generation;
  f.manager.set_agent_access(ID, false).unwrap();
  f.manager.set_agent_access(ID, true).unwrap();
  assert_eq!(
    f.manager
      .machine(ID, Actor::Agent)
      .unwrap()
      .agent_generation,
    initial + 2
  );
  assert!(old.send(InputDevice::Keyboard, events()).await.is_err());
  let mut next = connect(&f).await;
  next.send(InputDevice::Keyboard, events()).await.unwrap();
  next.close().await.unwrap();
  assert_eq!(f.trace().lines().filter(|line| *line == "input").count(), 2);
  f.sessions.stop(ID, Actor::Person).await.unwrap();
}

#[tokio::test]
async fn inactivity_and_server_teardown_release_input_without_stopping_the_vm() {
  let f = Fixture::new();
  f.start("normal").await;
  let server = f.sessions.serve_agents().unwrap();
  let mut old = control::connect(&f.manager, ID).await.unwrap();
  old.send(InputDevice::Keyboard, events()).await.unwrap();
  tokio::time::sleep(Duration::from_millis(5200)).await;
  assert!(old.send(InputDevice::Keyboard, events()).await.is_err());
  let mut next = connect(&f).await;
  next.send(InputDevice::Keyboard, events()).await.unwrap();
  drop(server);
  assert!(next.send(InputDevice::Keyboard, events()).await.is_err());
  tokio::time::timeout(Duration::from_secs(3), async {
    loop {
      if let Ok(person) = f.sessions.acquire_input(ID, Actor::Person).await {
        person.close().await.unwrap();
        break;
      }
      tokio::time::sleep(Duration::from_millis(5)).await;
    }
  })
  .await
  .unwrap();
  assert_eq!(
    f.sessions.state(ID, Actor::Person).await.unwrap(),
    Some(engine::machines::native::State::Running)
  );
  f.sessions.stop(ID, Actor::Person).await.unwrap();
}
