#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use machine::ipc;
use model::native::{Boot, Command, Request};
use std::{
  io::{Read, Write},
  process::{Child, Command as Process, Stdio},
  time::{Duration, Instant},
};

fn worker() -> Child {
  Process::new(env!("CARGO_BIN_EXE_hoppervm"))
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .unwrap()
}

fn failure(mut child: Child, message: &str) {
  drop(child.stdin.take());
  let deadline = Instant::now() + Duration::from_secs(3);
  let status = loop {
    if let Some(status) = child.try_wait().unwrap() {
      break status;
    }
    if Instant::now() >= deadline {
      child.kill().unwrap();
      child.wait().unwrap();
      panic!("native worker did not stop after invalid startup");
    }
    std::thread::sleep(Duration::from_millis(5));
  };
  assert!(!status.success());
  let mut stderr = String::new();
  child
    .stderr
    .take()
    .unwrap()
    .read_to_string(&mut stderr)
    .unwrap();
  assert!(stderr.contains(message), "{stderr}");
  let mut output = Vec::new();
  child
    .stdout
    .take()
    .unwrap()
    .read_to_end(&mut output)
    .unwrap();
  assert!(output.is_empty());
}

#[test]
fn incomplete_startup_stops_when_the_parent_disconnects() {
  let mut child = worker();
  child
    .stdin
    .as_mut()
    .unwrap()
    .write_all(b"HPV1\x01")
    .unwrap();
  failure(child, "Native parent disconnected");
}

#[test]
fn commands_cannot_allocate_a_vm_before_boot_configuration() {
  let mut child = worker();
  ipc::write_request(
    child.stdin.as_mut().unwrap(),
    &Request {
      id: 1,
      command: Command::Capture {},
    },
  )
  .unwrap();
  failure(child, "Native worker requires boot configuration first");
}

#[test]
fn invalid_hardware_does_not_create_persistent_state() {
  let temporary = tempfile::tempdir().unwrap();
  let firmware = temporary.path().join("firmware");
  let variables = temporary.path().join("variables");
  let store = temporary.path().join("store");
  std::fs::write(&firmware, 0x14000000u32.to_le_bytes()).unwrap();
  std::fs::write(&variables, [0xff]).unwrap();
  let mut child = worker();
  ipc::write_request(
    child.stdin.as_mut().unwrap(),
    &Request {
      id: 1,
      command: Command::Start {
        boot: Boot {
          firmware: firmware.to_string_lossy().into(),
          variables: variables.to_string_lossy().into(),
          store: store.to_string_lossy().into(),
          boot_media: None,
          installer: None,
          disk: None,
          disk_id: "hopper00000000000001".into(),
          memory: 0x10000000,
          cpus: 3,
          timeout_ms: Some(1000),
        },
      },
    },
  )
  .unwrap();
  failure(child, "Native runtime supports one or two CPUs");
  assert!(!store.exists());
}

#[test]
fn legacy_disk_headers_are_rejected_without_modifying_the_image() {
  let temporary = tempfile::tempdir().unwrap();
  let firmware = temporary.path().join("firmware");
  let variables = temporary.path().join("variables");
  let disk = temporary.path().join("disk");
  std::fs::write(&firmware, 0x14000000u32.to_le_bytes()).unwrap();
  std::fs::write(&variables, [0xff]).unwrap();
  let mut original = vec![0x5a; 512];
  original[..4].copy_from_slice(b"QFI\xfb");
  std::fs::write(&disk, &original).unwrap();
  let mut child = worker();
  ipc::write_request(
    child.stdin.as_mut().unwrap(),
    &Request {
      id: 1,
      command: Command::Start {
        boot: Boot {
          firmware: firmware.to_string_lossy().into(),
          variables: variables.to_string_lossy().into(),
          store: temporary.path().join("store").to_string_lossy().into(),
          boot_media: None,
          installer: None,
          disk: Some(disk.to_string_lossy().into()),
          disk_id: "hopper00000000000001".into(),
          memory: 0x10000000,
          cpus: 2,
          timeout_ms: Some(1000),
        },
      },
    },
  )
  .unwrap();
  failure(child, "Native guest disk requires raw format");
  assert_eq!(std::fs::read(&disk).unwrap(), original);
  assert!(!temporary.path().join("store").exists());
  let file = std::fs::OpenOptions::new()
    .read(true)
    .write(true)
    .open(disk)
    .unwrap();
  fs2::FileExt::try_lock_exclusive(&file).unwrap();
}
