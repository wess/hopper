use model::guest::{
  files::{self, Read, Stamp},
  Role,
};

fn request() -> Read {
  Read {
    path: "/home/hopper/file with ; $() spaces".into(),
    offset: 7,
    length: files::CHUNK,
    stamp: None,
    role: Role::Normal,
  }
}

#[test]
fn file_reads_build_literal_guest_commands_with_explicit_execution_identity() {
  let read = request();
  let command = read.command().unwrap();
  assert_eq!(command.executable, "/usr/libexec/hopperguest");
  assert_eq!(command.args, ["--read-file", &read.path, "7", "65536"]);
  assert_eq!(command.role, Role::Normal);
  assert_eq!(command.timeout_seconds, 30);
  assert!(command.valid_for(model::GuestOs::Linux));
  let mut read = read;
  read.role = Role::Administrator;
  let stamp = Stamp {
    device: 5,
    inode: 8,
    size: 80,
    modified: (1, 2),
    changed: (3, 4),
  };
  read.stamp = Some(stamp);
  let command = read.command().unwrap();
  assert_eq!(command.role, Role::Administrator);
  assert_eq!(
    serde_json::from_str::<Stamp>(&command.args[4]).unwrap(),
    stamp
  );
}

#[test]
fn invalid_paths_ranges_stamps_and_missing_roles_reject() {
  for path in [
    "relative".into(),
    "/contains\0nul".into(),
    format!("/{}", "x".repeat(4096)),
  ] {
    let mut read = request();
    read.path = path;
    assert!(read.command().is_none());
  }
  for (offset, length) in [(files::LIMIT + 1, 1), (0, 0), (0, files::CHUNK + 1)] {
    let mut read = request();
    read.offset = offset;
    read.length = length;
    assert!(read.command().is_none());
  }
  let mut read = request();
  read.stamp = Some(Stamp {
    device: 1,
    inode: 1,
    size: 6,
    modified: (0, 0),
    changed: (0, 0),
  });
  assert!(read.command().is_none());
  read.offset = 0;
  read.stamp.as_mut().unwrap().changed.1 = 1_000_000_000;
  assert!(read.command().is_none());
  let mut document = serde_json::to_value(request()).unwrap();
  document.as_object_mut().unwrap().remove("role");
  assert!(serde_json::from_value::<Read>(document).is_err());
}

#[test]
fn creation_payload_is_bounded_on_the_wire_and_absent_from_command_arguments() {
  use model::guest::{
    files::Create,
    wire::{Action, Request},
  };
  let mut file = Create {
    path: "/home/hopper/new file ; $()".into(),
    bytes: vec![255; files::CREATE_LIMIT],
    role: Role::Normal,
  };
  let command = file.command().unwrap();
  assert_eq!(command.args, ["--create-file", &file.path, "32768"]);
  assert!(command.valid_for(model::GuestOs::Linux));
  let request = Request {
    sequence: 1,
    action: Action::CreateFile { file: file.clone() },
  };
  assert!(request.encode(model::GuestOs::Linux).unwrap().len() < 256 << 10);
  assert!(!request.valid_for(model::GuestOs::Windows));
  file.bytes.push(0);
  assert!(file.command().is_none());
  file.bytes.clear();
  assert!(file.command().is_some());
  for path in [
    "/",
    "/home/hopper/",
    "/home/hopper/.",
    "/home/hopper/..",
    "relative",
  ] {
    file.path = path.into();
    assert!(file.command().is_none());
  }
}

#[test]
fn upload_metadata_is_bounded_and_checksum_does_not_enter_argv() {
  let mut upload = files::Upload {
    path: "/home/hopper/upload".into(),
    size: files::UPLOAD_LIMIT,
    sha256: "f".repeat(64),
    timeout_seconds: 600,
    role: Role::Normal,
  };
  let command = upload.command().unwrap();
  assert_eq!(
    command.args,
    ["--upload-file", "/home/hopper/upload", "1073741824"]
  );
  assert!(command.valid_for(model::GuestOs::Linux));
  upload.size += 1;
  assert!(upload.command().is_none());
  upload.size = 0;
  upload.sha256 = "g".repeat(64);
  assert!(upload.command().is_none());
  upload.sha256 = "f".repeat(64);
  upload.timeout_seconds = 0;
  assert!(upload.command().is_none());
}

#[test]
fn browsing_uses_literal_paths_bounded_byte_cursors_and_explicit_roles() {
  let mut request = model::guest::files::Browse {
    path: "/home/hopper;literal".into(),
    limit: 32,
    after: Some(vec![255; 255]),
    stamp: None,
    role: model::guest::Role::Normal,
  };
  let command = request.command().unwrap();
  assert!(command.valid_for(model::GuestOs::Linux));
  assert_eq!(command.args[0], "--browse-files");
  assert_eq!(command.args[1], request.path);
  for limit in [0, 33] {
    request.limit = limit;
    assert!(request.command().is_none());
  }
  request.limit = 32;
  for cursor in [
    Vec::new(),
    b"..".to_vec(),
    b"a/b".to_vec(),
    vec![0],
    vec![255; 256],
  ] {
    request.after = Some(cursor);
    assert!(request.command().is_none());
  }
  request.after = None;
  request.path = "/home/link/".into();
  assert!(request.command().is_none());
  let value = serde_json::json!({"path":"/home/hopper", "limit":1});
  assert!(serde_json::from_value::<model::guest::files::Browse>(value).is_err());
}
