use model::{Machine, MachineRuntime};

#[test]
fn previous_records_keep_their_implicit_runtime_and_new_choices_round_trip() {
  let mut document = serde_json::json!({
    "id": "00000000-0000-0000-0000-000000000001", "name": "Previous VM", "guest": "linux",
    "profile": "ubuntu", "resources": {"cpus": 2, "memoryGib": 4, "diskGib": 64},
    "agentAccess": true, "agentGeneration": 7,
  });
  let previous: Machine = serde_json::from_value(document.clone()).unwrap();
  assert_eq!(previous.runtime, None);
  assert_eq!(previous.agent_generation, 7);
  assert!(serde_json::to_value(previous)
    .unwrap()
    .get("runtime")
    .is_none());
  for (name, runtime) in [
    ("virtualization", MachineRuntime::Virtualization),
    ("hypervisor", MachineRuntime::Hypervisor),
  ] {
    document["runtime"] = name.into();
    let machine: Machine = serde_json::from_value(document.clone()).unwrap();
    assert_eq!(machine.runtime, Some(runtime));
    assert_eq!(serde_json::to_value(machine).unwrap()["runtime"], name);
  }
  document["runtime"] = "unknown".into();
  assert!(serde_json::from_value::<Machine>(document).is_err());
}
