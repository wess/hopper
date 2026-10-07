#![cfg(unix)]

use host::MachineActor;
use serde_json::{json, Value};
use std::sync::Arc;

fn content(result: &Value) -> &str {
  result["content"][0]["text"].as_str().unwrap()
}

#[tokio::test]
async fn windows_tools_use_native_records_and_never_dispatch_to_the_previous_runtime() {
  let root = tempfile::tempdir().unwrap();
  let previous = std::env::var_os("HOPPER_DIR");
  std::env::set_var("HOPPER_DIR", root.path());
  let host = Arc::new(host::Host::from_env());
  match previous {
    Some(value) => std::env::set_var("HOPPER_DIR", value),
    None => std::env::remove_var("HOPPER_DIR"),
  }
  let manager = host.machines();
  #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
  let machine = {
    let created = mcp::tools::call(
      &host,
      "vm.create",
      &json!({"name":"Native Windows", "profile":"windows"}),
    )
    .await;
    assert_ne!(created["isError"], true, "{created}");
    let machine: model::Machine = serde_json::from_str(content(&created)).unwrap();
    assert_eq!(machine.resources.cpus, 2);
    assert!(machine.agent_access);
    assert!(!manager.root.join("lima").exists());
    assert!(!manager.root.join("configs").exists());
    assert!(!manager.root.join("native").exists());
    machine
  };
  #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
  let machine = {
    let machine = model::Machine {
      id: model::new_uuid(),
      name: "Native Windows".into(),
      guest: model::GuestOs::Windows,
      profile: "windows".into(),
      resources: model::EngineResources {
        cpus: 2,
        memory_gib: 4,
        disk_gib: 64,
      },
      installer: None,
      agent_access: true,
    };
    std::fs::create_dir_all(manager.root.join("records")).unwrap();
    std::fs::write(
      manager
        .root
        .join("records")
        .join(format!("{}.json", machine.id)),
      serde_json::to_vec(&machine).unwrap(),
    )
    .unwrap();
    machine
  };
  let listed = mcp::tools::call(&host, "vm.list", &json!({})).await;
  assert_ne!(listed["isError"], true, "{listed}");
  let rows: Vec<model::MachineStatus> = serde_json::from_str(content(&listed)).unwrap();
  assert_eq!(rows.len(), 1);
  assert_eq!(rows[0].machine.id, machine.id);
  let legacy = manager.root.join("lima").join(&machine.id);
  std::fs::create_dir_all(&legacy).unwrap();
  let disk = legacy.join("diffdisk");
  std::fs::write(&disk, b"retained prior guest disk").unwrap();
  for tool in [
    "vm.start",
    "vm.stop",
    "vm.exec",
    "vm.input",
    "vm.screenshot",
    "vm.read_file",
    "vm.write_file",
    "vm.clone",
    "vm.snapshots",
    "vm.snapshot",
    "vm.restore",
  ] {
    let result = mcp::tools::call(&host, tool, &json!({"id":machine.id})).await;
    assert_eq!(result["isError"], true, "{tool}: {result}");
    assert!(
      content(&result).contains("Native Windows remote operations"),
      "{tool}: {result}"
    );
    assert_eq!(std::fs::read(&disk).unwrap(), b"retained prior guest disk");
  }
  manager.set_agent_access(&machine.id, false).unwrap();
  let listed = mcp::tools::call(&host, "vm.list", &json!({})).await;
  let rows: Vec<model::MachineStatus> = serde_json::from_str(content(&listed)).unwrap();
  assert!(rows.is_empty());
  let result = mcp::tools::call(&host, "vm.start", &json!({"id":machine.id})).await;
  assert_eq!(result["isError"], true);
  assert!(!content(&result).contains("Native Windows remote operations"));
  assert!(manager.machine(&machine.id, MachineActor::Agent).is_err());
}
