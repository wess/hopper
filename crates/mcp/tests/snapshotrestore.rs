#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[allow(dead_code)]
#[path = "../../engine/tests/snapshotcontroller/fixture.rs"]
mod fixture;
use engine::machines::{native::State, Actor};
use fixture::{Fixture, ID};
use serde_json::json;
use std::{fs, sync::Arc};
const SOURCE: &str = "02020202-0202-0202-0202-020202020202";

#[tokio::test]
async fn mcp_restore_reaches_external_owner_and_cancellation_drains_its_prior_capture() {
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
    mcp::tools::call(&caller, "vm.restore", &json!({"id":ID,"snapshot":SOURCE})).await
  });
  f.wait("snapshotpoll").await;
  task.abort();
  assert!(task.await.err().unwrap().is_cancelled());
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
  f.source.manager.set_agent_access(ID, false).unwrap();
  let before = f.trace();
  let response = mcp::tools::call(&host, "vm.restore", &json!({"id":ID,"snapshot":SOURCE})).await;
  assert_eq!(response["isError"], true, "{response}");
  assert_eq!(f.trace(), before);
  f.source.sessions.stop(ID, Actor::Person).await.unwrap();
}
