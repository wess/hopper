#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use machine::snapshot::{
  manifest::{Binding, Layout},
  package::{Description, Stage},
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
  fs::{File, Permissions},
  os::unix::fs::PermissionsExt,
  sync::Arc,
};

#[tokio::test]
async fn native_saved_snapshots_list_without_worker_and_enforce_access_and_identity() {
  let root = tempfile::tempdir_in("/tmp").unwrap();
  let previous = std::env::var_os("HOPPER_DIR");
  std::env::set_var("HOPPER_DIR", root.path());
  let host = Arc::new(host::Host::from_env());
  match previous {
    Some(value) => std::env::set_var("HOPPER_DIR", value),
    None => std::env::remove_var("HOPPER_DIR"),
  }
  let manager = host.machines();
  let id = model::new_uuid();
  let mut vm = model::Machine {
    id: id.clone(),
    name: "Snapshot fixture".into(),
    guest: model::GuestOs::Windows,
    profile: "windows".into(),
    resources: model::EngineResources {
      cpus: 2,
      memory_gib: 2,
      disk_gib: 64,
    },
    installer: None,
    agent_access: true,
    agent_generation: 0,
    runtime: Some(model::MachineRuntime::Hypervisor),
  };
  let record = manager.root.join("records").join(format!("{id}.json"));
  std::fs::create_dir_all(record.parent().unwrap()).unwrap();
  std::fs::write(&record, serde_json::to_vec(&vm).unwrap()).unwrap();
  let empty = mcp::tools::call(&host, "vm.snapshots", &json!({"id":id})).await;
  assert_ne!(empty["isError"], true, "{empty}");
  assert_eq!(empty["content"][0]["text"], "[]");
  let paths = engine::machines::native::config::initialize(&manager, &id).unwrap();
  let parent = paths.root.join("snapshots");
  std::fs::create_dir(&parent).unwrap();
  std::fs::set_permissions(&parent, Permissions::from_mode(0o700)).unwrap();
  let binding = Binding {
    machine: model::native::machine_identity(&id).unwrap(),
    snapshot: [2; 16],
    configuration: [3; 32],
  };
  let layout = Layout {
    cpus: 1,
    banks: [(0, 65536), (65536, 65536), (1 << 30, 65536)],
    devices: [0; 7],
  };
  let mut stage = Stage::new(&File::open(&parent).unwrap(), binding, layout, || Ok(())).unwrap();
  let mut bytes = b"test component".to_vec();
  bytes.extend_from_slice(&Sha256::digest(&bytes));
  for component in layout.records().unwrap() {
    stage
      .write(component, || Ok(()), |out| Ok(out.write_all(&bytes)?))
      .unwrap();
  }
  let mut sealed = stage.seal(|| Ok(())).unwrap();
  sealed
    .describe(
      &Description::new("Before updates", 1791822930).unwrap(),
      || Ok(()),
    )
    .unwrap();
  drop(sealed.publish(|| Ok(()), || Ok(())).unwrap());
  let response = mcp::tools::call(&host, "vm.snapshots", &json!({"id":id})).await;
  assert_ne!(response["isError"], true, "{response}");
  let entries: Value =
    serde_json::from_str(response["content"][0]["text"].as_str().unwrap()).unwrap();
  assert_eq!(entries.as_array().unwrap().len(), 1);
  assert_eq!(entries[0]["id"], "02020202-0202-0202-0202-020202020202");
  assert_eq!(entries[0]["machineId"], id);
  assert_eq!(entries[0]["configuration"], "03".repeat(32));
  assert_eq!(entries[0]["name"], "Before updates");
  assert_eq!(entries[0]["createdUnixSeconds"], 1791822930u64);
  manager.set_agent_access(&id, false).unwrap();
  let denied = mcp::tools::call(&host, "vm.snapshots", &json!({"id":id})).await;
  assert_eq!(denied["isError"], true);
  manager.set_agent_access(&id, true).unwrap();
  assert!(host
    .native_machine_snapshots_with_generation(&id, 0)
    .is_err());
  assert_eq!(
    host
      .native_machine_snapshots(&id, host::MachineActor::Person)
      .unwrap()
      .len(),
    1
  );
  vm.agent_generation = 2;
  vm.runtime = None;
  std::fs::write(&record, serde_json::to_vec(&vm).unwrap()).unwrap();
  let legacy = mcp::tools::call(&host, "vm.snapshots", &json!({"id":id})).await;
  assert_eq!(legacy["isError"], true);
  assert!(!manager.root.join("lima").exists());
  assert!(parent.join("02".repeat(16)).join("manifest.hsm").exists());
}
