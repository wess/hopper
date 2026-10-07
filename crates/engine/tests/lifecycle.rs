#![cfg(unix)]

#[allow(dead_code)]
#[path = "sessions/fixture.rs"]
mod fixture;

use engine::machines::{
  native::{
    sessions::remote::{self, control, Lifecycle},
    State,
  },
  Actor,
};
use fixture::{Fixture, ID};
use std::time::Duration;
use tokio::{
  io::{AsyncReadExt, AsyncWriteExt},
  net::UnixStream,
};

#[tokio::test]
async fn stale_and_unversioned_mutations_are_rejected_before_dispatch() {
  let f = Fixture::new();
  f.start("normal").await;
  let _server = f.sessions.serve_agents().unwrap();
  let generation = f
    .manager
    .machine(ID, Actor::Agent)
    .unwrap()
    .agent_generation;
  f.manager.set_agent_access(ID, false).unwrap();
  f.manager.set_agent_access(ID, true).unwrap();
  for operation in ["pause", "resume", "stop", "control"] {
    for version in [Some(generation), None] {
      let mut request = serde_json::json!({"vmId":ID, "operation":operation});
      if let Some(version) = version {
        request["agentGeneration"] = version.into();
      }
      let bytes = serde_json::to_vec(&request).unwrap();
      let mut stream = UnixStream::connect(f.manager.root.join("agents/native.sock"))
        .await
        .unwrap();
      stream.write_all(b"HPA1").await.unwrap();
      stream
        .write_all(&(bytes.len() as u32).to_le_bytes())
        .await
        .unwrap();
      stream.write_all(&bytes).await.unwrap();
      let mut prefix = [0; 8];
      tokio::time::timeout(Duration::from_secs(3), stream.read_exact(&mut prefix))
        .await
        .unwrap()
        .unwrap();
      assert_eq!(&prefix[..4], b"HPA1");
      let size = u32::from_le_bytes(prefix[4..].try_into().unwrap()) as usize;
      assert!(size <= 4096);
      let mut reply = vec![0; size];
      stream.read_exact(&mut reply).await.unwrap();
      let reply: serde_json::Value = serde_json::from_slice(&reply).unwrap();
      assert_eq!(reply["type"], "error", "{reply}");
    }
  }
  assert!(f.trace().is_empty());
  f.sessions.stop(ID, Actor::Person).await.unwrap();
}

#[tokio::test]
async fn remote_lifecycle_controls_the_existing_worker_and_ends_input_ownership() {
  let f = Fixture::new();
  f.start("normal").await;
  let _server = f.sessions.serve_agents().unwrap();
  let mut owner = control::connect(&f.manager, ID).await.unwrap();
  let watch = f.sessions.watch(ID).await.unwrap();
  remote::lifecycle(&f.manager, ID, Lifecycle::Pause)
    .await
    .unwrap();
  assert!(matches!(
    remote::status(&f.manager, ID).await.unwrap(),
    Some(remote::Status::Paused {})
  ));
  let model::native::Command::Input { device, events } = fixture::input() else {
    unreachable!()
  };
  assert!(owner.send(device, events).await.is_err());
  remote::lifecycle(&f.manager, ID, Lifecycle::Resume)
    .await
    .unwrap();
  assert!(matches!(
    remote::status(&f.manager, ID).await.unwrap(),
    Some(remote::Status::Running {})
  ));
  tokio::time::timeout(Duration::from_secs(3), async {
    loop {
      if let Ok(owner) = control::connect(&f.manager, ID).await {
        owner.close().await.unwrap();
        break;
      }
      tokio::time::sleep(Duration::from_millis(5)).await;
    }
  })
  .await
  .unwrap();
  remote::lifecycle(&f.manager, ID, Lifecycle::Stop)
    .await
    .unwrap();
  assert!(matches!(
    *watch.borrow(),
    State::Stopped(model::native::StopReason::Requested)
  ));
  assert!(f.sessions.state(ID, Actor::Person).await.unwrap().is_none());
  fixture::wait_lock(&f.lock(".runtime"), false).await;
  assert_eq!(
    std::fs::read_to_string(f.probe().join("boots"))
      .unwrap()
      .lines()
      .count(),
    1
  );
}

#[tokio::test]
async fn revoked_lifecycle_requests_do_not_change_hardware() {
  let f = Fixture::new();
  f.start("normal").await;
  let _server = f.sessions.serve_agents().unwrap();
  f.manager.set_agent_access(ID, false).unwrap();
  for action in [Lifecycle::Pause, Lifecycle::Resume, Lifecycle::Stop] {
    assert!(remote::lifecycle(&f.manager, ID, action).await.is_err());
  }
  assert!(f.trace().is_empty());
  assert_eq!(
    f.sessions.state(ID, Actor::Person).await.unwrap(),
    Some(State::Running)
  );
  f.manager.set_agent_access(ID, true).unwrap();
  remote::lifecycle(&f.manager, ID, Lifecycle::Stop)
    .await
    .unwrap();
}

#[tokio::test]
async fn cancelled_remote_stop_retains_ownership_until_worker_exit() {
  let f = Fixture::new();
  f.start("stopdelay").await;
  let _server = f.sessions.serve_agents().unwrap();
  let manager = f.manager.clone();
  let stop = tokio::spawn(async move { remote::lifecycle(&manager, ID, Lifecycle::Stop).await });
  f.marker("stopreceived").await;
  stop.abort();
  let _ = stop.await;
  fixture::wait_lock(&f.lock(".runtime"), true).await;
  f.signal("stopcontinue");
  fixture::wait_lock(&f.lock(".runtime"), true).await;
  f.signal("exitcontinue");
  fixture::wait_lock(&f.lock(".runtime"), false).await;
  f.start("normal").await;
  remote::lifecycle(&f.manager, ID, Lifecycle::Stop)
    .await
    .unwrap();
}
