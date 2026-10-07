#![cfg(unix)]

use host::MachineActor;
use serde_json::{json, Value};
use std::sync::Arc;

fn content(result: &Value) -> &str {
  result["content"][0]["text"].as_str().unwrap()
}

#[tokio::test]
async fn windows_tools_use_native_records_and_never_dispatch_to_the_previous_runtime() {
  let root = tempfile::tempdir_in("/tmp").unwrap();
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
      agent_generation: 0,
      runtime: Some(model::MachineRuntime::Hypervisor),
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
    "vm.exec",
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
      content(&result).contains("This native Windows operation"),
      "{tool}: {result}"
    );
    assert_eq!(std::fs::read(&disk).unwrap(), b"retained prior guest disk");
  }
  let missing = mcp::tools::call(&host, "vm.screenshot", &json!({"id":machine.id})).await;
  assert_eq!(missing["isError"], true);
  for tool in ["vm.stop", "vm.pause", "vm.resume"] {
    let missing = mcp::tools::call(&host, tool, &json!({"id":machine.id})).await;
    assert_eq!(missing["isError"], true, "{missing}");
    assert_eq!(std::fs::read(&disk).unwrap(), b"retained prior guest disk");
  }
  let mut native = machine.clone();
  native.id = model::new_uuid();
  std::fs::write(
    manager
      .root
      .join("records")
      .join(format!("{}.json", native.id)),
    serde_json::to_vec(&native).unwrap(),
  )
  .unwrap();
  let probe = root.path().join("probe");
  std::fs::create_dir(&probe).unwrap();
  let worker = root.path().join("worker");
  std::fs::write(&worker, include_str!("../../engine/tests/native/worker.py")).unwrap();
  use std::os::unix::fs::PermissionsExt;
  std::fs::set_permissions(&worker, std::fs::Permissions::from_mode(0o700)).unwrap();
  host.serve_machine_agents().unwrap();
  host
    .native_machines()
    .start(
      &native.id,
      &worker,
      model::native::Boot {
        firmware: String::new(),
        variables: String::new(),
        store: probe.to_string_lossy().into(),
        boot_media: None,
        installer: None,
        disk: None,
        disk_id: "normal".into(),
        memory: 0x10000000,
        cpus: 2,
        timeout_ms: None,
      },
    )
    .await
    .unwrap();
  let image = mcp::tools::call(&host, "vm.screenshot", &json!({"id":native.id})).await;
  assert_ne!(image["isError"], true, "{image}");
  use base64::Engine;
  let bytes = base64::engine::general_purpose::STANDARD
    .decode(image["content"][0]["data"].as_str().unwrap())
    .unwrap();
  assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
  let pixels = image::load_from_memory(&bytes).unwrap().into_rgba8();
  assert_eq!(pixels.dimensions(), (1, 1));
  assert_eq!(pixels.as_raw(), &[1, 2, 3, 255]);
  use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
  let mut client = tokio::process::Command::new(env!("CARGO_BIN_EXE_hoppermcp"))
    .env("HOPPER_DIR", root.path())
    .stdin(std::process::Stdio::piped())
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::null())
    .kill_on_drop(true)
    .spawn()
    .unwrap();
  let mut input = client.stdin.take().unwrap();
  let mut output = tokio::io::BufReader::new(client.stdout.take().unwrap());
  let requests = [
    ("vm.screenshot", json!({"id":native.id}), false),
    ("vm.pause", json!({"id":native.id}), false),
    ("vm.resume", json!({"id":native.id}), false),
    (
      "vm.input",
      json!({"id":native.id,"input":{"type":"key","keys":"ctrl+alt+delete"}}),
      false,
    ),
    (
      "vm.input",
      json!({"id":native.id,"input":{"type":"pointer","x":32767,"y":0,"button":"left"}}),
      false,
    ),
    ("vm.screenshot", json!({"id":native.id}), true),
    (
      "vm.input",
      json!({"id":native.id,"input":{"type":"key","keys":"a"}}),
      true,
    ),
    ("vm.stop", json!({"id":native.id}), true),
    ("vm.stop", json!({"id":native.id}), false),
  ];
  for (index, (name, arguments, denied)) in requests.into_iter().enumerate() {
    manager.set_agent_access(&native.id, !denied).unwrap();
    let number = index + 1;
    let request = json!({"jsonrpc":"2.0", "id":number, "method":"tools/call", "params":{"name":name, "arguments":arguments}});
    input
      .write_all(format!("{request}\n").as_bytes())
      .await
      .unwrap();
    input.flush().await.unwrap();
    let mut line = String::new();
    tokio::time::timeout(
      std::time::Duration::from_secs(5),
      output.read_line(&mut line),
    )
    .await
    .unwrap()
    .unwrap();
    let response: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(response["id"], number);
    if denied {
      assert_eq!(response["result"]["isError"], true);
    } else if name == "vm.screenshot" {
      let bytes = base64::engine::general_purpose::STANDARD
        .decode(response["result"]["content"][0]["data"].as_str().unwrap())
        .unwrap();
      assert_eq!(
        image::load_from_memory(&bytes)
          .unwrap()
          .into_rgba8()
          .as_raw(),
        &[1, 2, 3, 255]
      );
    } else {
      assert_ne!(response["result"]["isError"], true, "{response}");
    }
  }
  let inputs: Vec<Value> = std::fs::read_to_string(probe.join("inputs"))
    .unwrap()
    .lines()
    .map(|line| serde_json::from_str(line).unwrap())
    .collect();
  assert_eq!(inputs.len(), 2);
  assert_eq!(inputs[0]["device"], "keyboard");
  assert_eq!(
    inputs[0]["events"],
    json!([
      {"kind":1,"code":29,"value":1},{"kind":1,"code":56,"value":1},{"kind":1,"code":111,"value":1},
      {"kind":0,"code":0,"value":0},
      {"kind":1,"code":111,"value":0},{"kind":1,"code":56,"value":0},{"kind":1,"code":29,"value":0},
      {"kind":0,"code":0,"value":0},
    ])
  );
  assert_eq!(inputs[1]["device"], "tablet");
  assert_eq!(inputs[1]["events"][0]["value"], 65535);
  assert_eq!(inputs[1]["events"][1]["value"], 0);
  assert_eq!(
    inputs[1]["events"][2],
    json!({"kind":1,"code":272,"value":1})
  );
  assert_eq!(
    inputs[1]["events"][4],
    json!({"kind":1,"code":272,"value":0})
  );
  drop(input);
  assert!(
    tokio::time::timeout(std::time::Duration::from_secs(5), client.wait())
      .await
      .unwrap()
      .unwrap()
      .success()
  );
  assert!(host
    .native_machines()
    .state(&native.id, MachineActor::Person)
    .await
    .unwrap()
    .is_none());
  assert!(std::fs::read_to_string(probe.join("trace"))
    .unwrap()
    .ends_with("stop\n"));
  manager.set_agent_access(&native.id, false).unwrap();
  let denied = mcp::tools::call(&host, "vm.screenshot", &json!({"id":native.id})).await;
  assert_eq!(denied["isError"], true);
  manager.set_agent_access(&machine.id, false).unwrap();
  let listed = mcp::tools::call(&host, "vm.list", &json!({})).await;
  let rows: Vec<model::MachineStatus> = serde_json::from_str(content(&listed)).unwrap();
  assert!(rows.is_empty());
  let result = mcp::tools::call(&host, "vm.start", &json!({"id":machine.id})).await;
  assert_eq!(result["isError"], true);
  assert!(!content(&result).contains("This native Windows operation"));
  assert!(manager.machine(&machine.id, MachineActor::Agent).is_err());
}
