use model::native::{machine_identity, Result as Reply};

#[test]
fn startup_receipt_requires_the_canonical_wire_field() {
  let identity = "01010101-0101-0101-0101-010101010101";
  let receipt = Reply::Started {
    machine_id: identity.into(),
    configuration: [7; 32],
  };
  let value = serde_json::to_value(receipt).unwrap();
  assert_eq!(
    value,
    serde_json::json!({"type": "started", "machineId": identity, "configuration": vec![7; 32]})
  );
  assert!(
    matches!(serde_json::from_value::<Reply>(value).unwrap(), Reply::Started { machine_id, .. } if machine_id == identity)
  );
  for value in [
    serde_json::json!({"type": "started"}),
    serde_json::json!({"type": "started", "machineId": identity}),
    serde_json::json!({"type": "started", "machineId": identity, "configuration": vec![7; 31]}),
    serde_json::json!({"type": "started", "machineId": identity, "configuration": vec![7; 33]}),
    serde_json::json!({"type": "started", "machineId": identity, "configuration": vec![256; 32]}),
    serde_json::json!({"type": "started", "machine_id": identity}),
    serde_json::json!({"type": "started", "machineId": identity, "extra": true}),
  ] {
    assert!(serde_json::from_value::<Reply>(value).is_err());
  }
}

#[test]
fn native_identity_requires_nonzero_canonical_uuid() {
  assert_eq!(
    machine_identity("01010101-0101-0101-0101-010101010101"),
    Some([1; 16])
  );
  for identity in [
    "",
    "../host",
    "00000000-0000-0000-0000-000000000000",
    "A1010101-0101-0101-0101-010101010101",
    "01010101010101010101010101010101",
  ] {
    assert!(machine_identity(identity).is_none());
  }
}

#[test]
fn snapshot_commands_require_original_generation_and_accept_no_host_paths() {
  for kind in ["snapshotStart", "snapshotPoll", "snapshotCancel"] {
    let valid = serde_json::json!({"type": kind,
      "snapshotId": "02020202-0202-0202-0202-020202020202", "policyGeneration": 7});
    assert!(serde_json::from_value::<model::native::Command>(valid.clone()).is_ok());
    for fault in 0..4 {
      let mut value = valid.clone();
      match fault {
        0 => {
          value.as_object_mut().unwrap().remove("policyGeneration");
        }
        1 => value["policyGeneration"] = serde_json::json!(-1),
        2 => value["path"] = serde_json::json!("/host/private"),
        _ => {
          value.as_object_mut().unwrap().remove("snapshotId");
        }
      }
      assert!(serde_json::from_value::<model::native::Command>(value).is_err());
    }
  }
}

#[test]
fn snapshot_receipts_have_fixed_fields_and_no_path_capability() {
  use model::native::snapshot::{Receipt, Status};
  let receipt = Receipt {
    nonce: [1; 16],
    device: 2,
    inode: 3,
    manifest: [4; 32],
  };
  let reply = Reply::Snapshot {
    snapshot_id: "02020202-0202-0202-0202-020202020202".into(),
    status: Status::Ready { receipt },
  };
  let value = serde_json::to_value(reply).unwrap();
  assert_eq!(value["snapshotId"], "02020202-0202-0202-0202-020202020202");
  assert_eq!(value["status"]["state"], "ready");
  assert!(serde_json::from_value::<Reply>(value.clone()).is_ok());
  for fault in 0..4 {
    let mut invalid = value.clone();
    match fault {
      0 => invalid["status"]["receipt"]["nonce"] = serde_json::json!(vec![1; 15]),
      1 => invalid["status"]["receipt"]["manifest"] = serde_json::json!(vec![1; 33]),
      2 => invalid["status"]["receipt"]["path"] = serde_json::json!("../host"),
      _ => invalid["status"]["receipt"]["inode"] = serde_json::json!(-1),
    }
    assert!(serde_json::from_value::<Reply>(invalid).is_err());
  }
}

#[test]
fn restore_activation_requires_explicit_canonical_field_names_and_policy() {
  for kind in ["restoreResume", "restorePause"] {
    let valid = serde_json::json!({"type":kind, "restore": {
      "snapshotId":"02020202-0202-0202-0202-020202020202",
      "generationId":"04040404-0404-0404-0404-040404040404", "policyGeneration":7
    }});
    assert!(serde_json::from_value::<model::native::Command>(valid.clone()).is_ok());
    for fault in 0..5 {
      let mut value = valid.clone();
      match fault {
        0 => {
          value["restore"]
            .as_object_mut()
            .unwrap()
            .remove("policyGeneration");
        }
        1 => value["restore"]["policyGeneration"] = serde_json::json!(-1),
        2 => value["restore"]["path"] = serde_json::json!("/host/private"),
        3 => {
          value["restore"]
            .as_object_mut()
            .unwrap()
            .remove("generationId");
        }
        _ => {
          value["restore"]["policy_generation"] = serde_json::json!(7);
        }
      }
      assert!(serde_json::from_value::<model::native::Command>(value).is_err());
    }
  }
}

#[test]
fn generation_start_has_a_distinct_receipt_and_requires_original_policy() {
  let boot = serde_json::json!({"machineId":"01010101-0101-0101-0101-010101010101",
    "firmware":"/private/firmware", "variables":"/private/template", "store":"/private/vm/variables",
    "diskId":"hopper00000000000001", "memory": 268435456, "cpus":2});
  let restore = serde_json::json!({"snapshotId":"02020202-0202-0202-0202-020202020202",
    "generationId":"04040404-0404-0404-0404-040404040404", "policyGeneration":7});
  let command = serde_json::json!({"type":"startGeneration", "boot":boot, "restore":restore});
  assert!(matches!(
    serde_json::from_value::<model::native::Command>(command.clone()).unwrap(),
    model::native::Command::StartGeneration { .. }
  ));
  let reply = serde_json::json!({"type":"generationStarted", "machineId":"01010101-0101-0101-0101-010101010101",
    "configuration": vec![3;32], "restore": command["restore"]});
  assert!(matches!(
    serde_json::from_value::<Reply>(reply.clone()).unwrap(),
    Reply::GenerationStarted { .. }
  ));
  for fault in 0..4 {
    let mut invalid = command.clone();
    match fault {
      0 => {
        invalid["restore"]
          .as_object_mut()
          .unwrap()
          .remove("policyGeneration");
      }
      1 => invalid["restore"]["path"] = serde_json::json!("/host/other"),
      2 => {
        invalid.as_object_mut().unwrap().remove("boot");
      }
      _ => invalid["unexpected"] = serde_json::json!(true),
    }
    assert!(serde_json::from_value::<model::native::Command>(invalid).is_err());
    let mut invalid = reply.clone();
    match fault {
      0 => {
        invalid["restore"]
          .as_object_mut()
          .unwrap()
          .remove("policyGeneration");
      }
      1 => invalid["configuration"] = serde_json::json!(vec![3; 31]),
      2 => {
        invalid.as_object_mut().unwrap().remove("machineId");
      }
      _ => invalid["unexpected"] = serde_json::json!(true),
    }
    assert!(serde_json::from_value::<Reply>(invalid).is_err());
  }
}
