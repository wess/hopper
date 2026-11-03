use model::guest::{Command, Role};

fn command() -> Command {
  Command {
    executable: "/bin/echo".into(),
    args: vec!["literal; id".into()],
    directory: None,
    timeout_seconds: 60,
    role: Role::Normal,
  }
}

#[test]
fn guest_commands_require_explicit_executables_roles_and_bounded_literal_arguments() {
  let mut request = command();
  assert!(request.valid_for(model::GuestOs::Linux));
  request.executable = "sh -c id".into();
  assert!(!request.valid_for(model::GuestOs::Linux));
  request = command();
  request.args = vec!["x".repeat(4096); 9];
  assert!(!request.valid_for(model::GuestOs::Linux));
  request = command();
  request.args = vec!["".into(); 129];
  assert!(!request.valid_for(model::GuestOs::Linux));
  request = command();
  request.args = vec!["bad\0argument".into()];
  assert!(!request.valid_for(model::GuestOs::Linux));
  request = command();
  request.directory = Some("relative".into());
  assert!(!request.valid_for(model::GuestOs::Linux));
  request = command();
  request.timeout_seconds = 601;
  assert!(!request.valid_for(model::GuestOs::Linux));
  request.timeout_seconds = 0;
  assert!(!request.valid_for(model::GuestOs::Linux));
}

#[test]
fn guest_wire_contract_rejects_extra_identity_or_environment_overrides() {
  let mut value = serde_json::to_value(command()).unwrap();
  value["uid"] = 0.into();
  assert!(serde_json::from_value::<Command>(value).is_err());
  let mut value = serde_json::to_value(command()).unwrap();
  value["env"] = serde_json::json!({"HOME": "/root"});
  assert!(serde_json::from_value::<Command>(value).is_err());
}

#[test]
fn guest_paths_use_the_selected_os_and_allow_literal_names_on_its_search_path() {
  let mut request = command();
  request.executable = "id".into();
  assert!(request.valid_for(model::GuestOs::Linux));
  request.executable = "C:\\Windows\\System32\\cmd.exe".into();
  request.directory = Some("C:\\Users\\hopper".into());
  assert!(request.valid_for(model::GuestOs::Windows));
  assert!(!request.valid_for(model::GuestOs::Linux));
  request.executable = "../bin/id".into();
  assert!(!request.valid_for(model::GuestOs::Linux));
}
