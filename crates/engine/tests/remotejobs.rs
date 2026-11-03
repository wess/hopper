#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

#[allow(dead_code)]
#[path = "sessions/fixture.rs"]
mod fixture;

use engine::machines::{native::sessions::remote::jobs, Actor};
use fixture::{Fixture, ID};
use model::{guest::wire::Action, GuestOs, MachineRuntime};
use serde_json::{json, Value};
use std::time::Duration;
use tokio::{
  io::{AsyncReadExt, AsyncWriteExt},
  net::UnixStream,
};

fn linux(f: &Fixture) {
  let mut machine = f.manager.machine(ID, Actor::Person).unwrap();
  machine.guest = GuestOs::Linux;
  machine.profile = "ubuntu".into();
  machine.runtime = Some(MachineRuntime::Virtualization);
  store::json::write(
    &f.manager.root.join("records").join(format!("{ID}.json")),
    &machine,
  )
  .unwrap();
}

async fn header(f: &Fixture, value: Value) -> Value {
  let mut stream = UnixStream::connect(f.manager.root.join("agents/native.sock"))
    .await
    .unwrap();
  let bytes = serde_json::to_vec(&value).unwrap();
  stream.write_all(b"HPA1").await.unwrap();
  stream
    .write_all(&(bytes.len() as u32).to_le_bytes())
    .await
    .unwrap();
  stream.write_all(&bytes).await.unwrap();
  let mut prefix = [0; 8];
  tokio::time::timeout(Duration::from_secs(1), stream.read_exact(&mut prefix))
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
async fn guest_socket_reaches_the_virtual_owner_without_allocating_or_booting() {
  let f = Fixture::new();
  linux(&f);
  let (client, mut owner) = engine::machines::vz::channel();
  f.sessions.connect_virtual_agents(client.clone()).unwrap();
  assert!(f.sessions.connect_virtual_agents(client).is_err());
  let _server = f.sessions.serve_agents().unwrap();
  let manager = f.manager.clone();
  let task = tokio::spawn(async move {
    jobs::request(
      &manager,
      ID,
      Action::Poll {
        job: "4ae3943e-9d15-44f2-b0f5-5729bc464c0f".into(),
      },
    )
    .await
  });
  tokio::time::timeout(Duration::from_secs(2), owner.wake().notified())
    .await
    .unwrap();
  owner.tick();
  let error = task.await.unwrap().unwrap_err();
  assert!(error.to_string().contains("not owned"), "{error:#}");
  assert!(!owner.active());
  assert!(!f.manager.root.join("vz").exists());
  assert!(!f.manager.root.join("lima").exists());
  assert!(!f.probe().join("boots").exists());
}

#[tokio::test]
async fn guest_socket_rejects_missing_policy_wrong_family_and_oversized_body_before_reading_it() {
  let f = Fixture::new();
  let _server = f.sessions.serve_agents().unwrap();
  let reply = header(
    &f,
    json!({"vmId":ID, "operation":{"guest":{"length":100}}, "agentGeneration":0}),
  )
  .await;
  assert_eq!(reply["type"], "error");
  assert!(reply["message"].as_str().unwrap().contains("native Linux"));
  linux(&f);
  let reply = header(&f, json!({"vmId":ID, "operation":{"guest":{"length":100}}})).await;
  assert!(reply["message"].as_str().unwrap().contains("generation"));
  let reply = header(
    &f,
    json!({"vmId":ID, "operation":{"guest":{"length":(256 << 10) + 9}}, "agentGeneration":0}),
  )
  .await;
  assert!(reply["message"].as_str().unwrap().contains("bounds"));
  f.manager.set_agent_access(ID, false).unwrap();
  let reply = header(
    &f,
    json!({"vmId":ID, "operation":{"guest":{"length":100}}, "agentGeneration":0}),
  )
  .await;
  assert_eq!(reply["type"], "error");
  assert!(!f.probe().join("boots").exists());
}
