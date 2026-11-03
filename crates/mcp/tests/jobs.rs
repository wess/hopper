#![cfg(unix)]

use serde_json::{json, Value};
use std::sync::Arc;

#[test]
fn catalogue_requires_literal_commands_explicit_roles_and_bounded_timeouts() {
  let catalogue = mcp::tools::catalogue();
  let tools = catalogue["tools"].as_array().unwrap();
  let start = tools
    .iter()
    .find(|tool| tool["name"] == "vm.job_start")
    .unwrap();
  let schema = &start["inputSchema"]["properties"]["command"];
  assert_eq!(schema["additionalProperties"], false);
  assert_eq!(
    schema["properties"]["role"]["enum"],
    json!(["normal", "administrator"])
  );
  assert_eq!(schema["properties"]["timeoutSeconds"]["maximum"], 600);
  assert!(schema["required"]
    .as_array()
    .unwrap()
    .contains(&json!("role")));
  for name in [
    "vm.job_poll",
    "vm.job_cancel",
    "vm.upload_begin",
    "vm.upload_chunk",
    "vm.upload_status",
    "vm.upload_commit",
    "vm.file_browse",
  ] {
    assert!(tools.iter().any(|tool| tool["name"] == name));
  }
  let read = tools
    .iter()
    .find(|tool| tool["name"] == "vm.file_read")
    .unwrap();
  let file = &read["inputSchema"]["properties"]["file"];
  assert_eq!(file["additionalProperties"], false);
  assert_eq!(file["properties"]["length"]["maximum"], 65536);
  let create = tools
    .iter()
    .find(|tool| tool["name"] == "vm.file_create")
    .unwrap();
  assert_eq!(
    create["inputSchema"]["properties"]["file"]["properties"]["bytes"]["maxItems"],
    32768
  );
  let upload = tools
    .iter()
    .find(|tool| tool["name"] == "vm.upload_begin")
    .unwrap();
  let upload = &upload["inputSchema"]["properties"]["file"];
  assert_eq!(upload["properties"]["size"]["maximum"], 1 << 30);
  assert!(upload["required"]
    .as_array()
    .unwrap()
    .contains(&json!("role")));
  let chunk = tools
    .iter()
    .find(|tool| tool["name"] == "vm.upload_chunk")
    .unwrap();
  assert_eq!(
    chunk["inputSchema"]["properties"]["bytes"]["maxItems"],
    32768
  );
  assert!(file["required"]
    .as_array()
    .unwrap()
    .contains(&json!("role")));
}

#[tokio::test]
async fn native_jobs_authorize_and_validate_without_using_the_previous_helper() {
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
  let mut machine = model::Machine {
    id: id.clone(),
    name: "Jobs fixture".into(),
    guest: model::GuestOs::Linux,
    profile: "ubuntu".into(),
    resources: model::EngineResources {
      cpus: 2,
      memory_gib: 2,
      disk_gib: 64,
    },
    installer: None,
    agent_access: true,
    agent_generation: 0,
    runtime: Some(model::MachineRuntime::Virtualization),
  };
  let record = manager.root.join("records").join(format!("{id}.json"));
  std::fs::create_dir_all(record.parent().unwrap()).unwrap();
  std::fs::write(&record, serde_json::to_vec(&machine).unwrap()).unwrap();
  let command = json!({"executable":"printf", "args":["literal; argument"], "role":"normal", "timeoutSeconds":10});
  for directory in [
    json!({"path":"/home/hopper", "limit":32, "role":"normal"}),
    json!({"path":"relative", "limit":1, "role":"normal"}),
    json!({"path":"/home/hopper", "limit":33, "role":"normal"}),
    json!({"path":"/home/hopper", "limit":1}),
  ] {
    let result = mcp::tools::call(
      &host,
      "vm.file_browse",
      &json!({"id":id, "directory":directory}),
    )
    .await;
    assert_eq!(result["isError"], true, "{result}");
  }
  for (name, args) in [
    (
      "vm.upload_begin",
      json!({"id":id, "file":{"path":"/home/hopper/new", "size":3, "sha256":"ab".repeat(32), "role":"normal", "timeoutSeconds":30}}),
    ),
    (
      "vm.upload_begin",
      json!({"id":id, "file":{"path":"/home/hopper/new", "size":3, "sha256":"bad", "role":"normal", "timeoutSeconds":30}}),
    ),
    (
      "vm.upload_chunk",
      json!({"id":id, "job":model::new_uuid(), "offset":0, "bytes":[0,255]}),
    ),
    (
      "vm.upload_chunk",
      json!({"id":id, "job":model::new_uuid(), "offset":0, "bytes":[256]}),
    ),
    (
      "vm.upload_status",
      json!({"id":id, "job":model::new_uuid()}),
    ),
    (
      "vm.upload_commit",
      json!({"id":id, "job":model::new_uuid()}),
    ),
  ] {
    let result = mcp::tools::call(&host, name, &args).await;
    assert_eq!(result["isError"], true, "{result}");
  }
  for command in [
    command.clone(),
    json!({"executable":"printf", "args":[], "timeoutSeconds":10}),
    json!({"executable":"printf", "args":[], "role":"normal", "timeoutSeconds":10, "environment":{"HOST":"value"}}),
  ] {
    let result =
      mcp::tools::call(&host, "vm.job_start", &json!({"id":id, "command":command})).await;
    assert_eq!(result["isError"], true, "{result}");
  }
  machine.agent_access = false;
  for file in [
    json!({"path":"/home/hopper/new", "bytes":[0,255], "role":"normal"}),
    json!({"path":"relative", "bytes":[], "role":"normal"}),
    json!({"path":"/home/hopper/new", "bytes":[256], "role":"normal"}),
    json!({"path":"/home/hopper/new", "bytes":[]}),
  ] {
    let result = mcp::tools::call(&host, "vm.file_create", &json!({"id":id, "file":file})).await;
    assert_eq!(result["isError"], true, "{result}");
  }
  for file in [
    json!({"path":"/home/hopper/data", "offset":0, "length":64, "role":"normal"}),
    json!({"path":"relative", "offset":0, "length":64, "role":"normal"}),
    json!({"path":"/home/hopper/data", "offset":0, "length":65537, "role":"normal"}),
    json!({"path":"/home/hopper/data", "offset":0, "length":64}),
  ] {
    let result = mcp::tools::call(&host, "vm.file_read", &json!({"id":id, "file":file})).await;
    assert_eq!(result["isError"], true, "{result}");
  }
  std::fs::write(&record, serde_json::to_vec(&machine).unwrap()).unwrap();
  let result = mcp::tools::call(&host, "vm.job_start", &json!({"id":id, "command":command})).await;
  assert_eq!(result["isError"], true);
  let result = mcp::tools::call(
    &host,
    "vm.file_read",
    &json!({"id":id, "file":{
      "path":"/home/hopper/data", "offset":0, "length":64, "role":"normal"
    }}),
  )
  .await;
  assert_eq!(result["isError"], true);
  assert!(!manager.root.join("lima").exists());
  let result = mcp::tools::call(
    &host,
    "vm.file_create",
    &json!({"id":id, "file":{
      "path":"/home/hopper/new", "bytes":[0], "role":"normal"
    }}),
  )
  .await;
  assert_eq!(result["isError"], true);
  assert!(!manager.root.join("native").exists());
  assert!(!manager.root.join("agents").exists());
  assert!(!manager.root.join("vz").exists());
  let preserved: Value = serde_json::from_slice(&std::fs::read(record).unwrap()).unwrap();
  assert_eq!(preserved["agentAccess"], false);
}
