#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[allow(dead_code)]
#[path = "../../engine/tests/snapshotcontroller/fixture.rs"]
mod fixture;
use fixture::{Fixture, ID};
use serde_json::{json, Value};
use std::sync::Arc;

#[tokio::test]
async fn mcp_snapshot_reaches_external_owner_and_lists_the_named_committed_package() {
  let f = Fixture::new("snapshotgates").await;
  let _server = f.source.sessions.serve_agents().unwrap();
  let previous = std::env::var_os("HOPPER_DIR");
  std::env::set_var("HOPPER_DIR", f.source.root.path());
  let host = Arc::new(host::Host::from_env());
  match previous {
    Some(value) => std::env::set_var("HOPPER_DIR", value),
    None => std::env::remove_var("HOPPER_DIR"),
  }
  assert_eq!(host.machines().root, f.source.manager.root);
  let caller = host.clone();
  let task = tokio::spawn(async move {
    mcp::tools::call(
      &caller,
      "vm.snapshot",
      &json!({"id":ID,"name":" Before updates "}),
    )
    .await
  });
  f.wait("snapshotpoll").await;
  let snapshot = std::fs::read_to_string(f.variables.join("snapshotidentity")).unwrap();
  f.ready(&snapshot);
  f.marker("pollcontinue");
  f.wait("snapshotcancel").await;
  assert!(f.locked());
  f.marker("cancelcontinue");
  let response = task.await.unwrap();
  assert_ne!(response["isError"], true, "{response}");
  let receipt: Value =
    serde_json::from_str(response["content"][0]["text"].as_str().unwrap()).unwrap();
  assert_eq!(receipt["id"], snapshot);
  assert_eq!(receipt["machineId"], ID);
  assert_eq!(receipt["name"], "Before updates");
  assert!(!f.locked());
  let response = mcp::tools::call(&host, "vm.snapshots", &json!({"id":ID})).await;
  assert_ne!(response["isError"], true, "{response}");
  let entries: Value =
    serde_json::from_str(response["content"][0]["text"].as_str().unwrap()).unwrap();
  assert_eq!(entries.as_array().unwrap().len(), 1);
  assert_eq!(entries[0]["id"], snapshot);
  assert_eq!(entries[0]["name"], "Before updates");
  assert!(entries[0]["createdUnixSeconds"].as_u64().unwrap() > 0);
  f.source
    .sessions
    .stop(ID, engine::machines::Actor::Person)
    .await
    .unwrap();
}
