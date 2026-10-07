#![cfg(unix)]

#[allow(dead_code)]
#[path = "sessions/fixture.rs"]
mod fixture;

use engine::machines::{native::sessions::remote, Actor};
use fixture::{Fixture, ID};
use std::{os::unix::fs::PermissionsExt, time::Duration};
use tokio::{
  io::{AsyncReadExt, AsyncWriteExt},
  net::UnixStream,
};

#[tokio::test]
async fn remote_capture_reaches_the_owned_worker_without_another_boot() {
  let f = Fixture::new();
  f.start("normal").await;
  let server = f.sessions.serve_agents().unwrap();
  assert!(f.sessions.serve_agents().is_err());
  assert!(matches!(
    remote::status(&f.manager, ID).await.unwrap(),
    Some(remote::Status::Running {})
  ));
  let frame = remote::capture(&f.manager, ID).await.unwrap();
  assert_eq!((frame.width, frame.height, frame.generation), (1, 1, 7));
  assert_eq!(frame.rgba, [1, 2, 3, 255]);
  let endpoint = f.manager.root.join("agents/native.sock");
  assert_eq!(
    std::fs::metadata(&endpoint).unwrap().permissions().mode() & 0o777,
    0o600
  );
  let script = r#"import json, socket, struct, sys
s = socket.socket(socket.AF_UNIX)
s.connect(sys.argv[1])
def read(n):
    data = b''
    while len(data) < n:
        part = s.recv(n - len(data))
        assert part
        data += part
    return data
h = json.dumps({'vmId': sys.argv[2], 'operation': 'capture'}).encode()
s.sendall(b'HPA1' + struct.pack('<I', len(h)) + h)
prefix = read(8)
assert prefix[:4] == b'HPA1'
reply = json.loads(read(struct.unpack('<I', prefix[4:])[0]))
assert reply == {'type': 'frame', 'width': 1, 'height': 1, 'generation': 7}
assert read(4) == bytes([1, 2, 3, 255])
"#;
  let output = tokio::process::Command::new("python3")
    .arg("-c")
    .arg(script)
    .arg(&endpoint)
    .arg(ID)
    .output()
    .await
    .unwrap();
  assert!(
    output.status.success(),
    "{}",
    String::from_utf8_lossy(&output.stderr)
  );
  assert_eq!(
    std::fs::read_to_string(f.probe().join("boots"))
      .unwrap()
      .lines()
      .count(),
    1
  );
  drop(server);
  assert!(!endpoint.exists());
  assert!(remote::capture(&f.manager, ID).await.is_err());
  assert_eq!(
    f.sessions.state(ID, Actor::Person).await.unwrap(),
    Some(engine::machines::native::State::Running)
  );
  f.sessions.stop(ID, Actor::Person).await.unwrap();
  let server = f.sessions.serve_agents().unwrap();
  drop(server);
}

#[tokio::test]
async fn policy_revocation_discards_a_frame_already_in_flight() {
  let f = Fixture::new();
  f.start("cancel").await;
  let _server = f.sessions.serve_agents().unwrap();
  let manager = f.manager.clone();
  let capture = tokio::spawn(async move { remote::capture(&manager, ID).await });
  f.marker("partial").await;
  f.manager.set_agent_access(ID, false).unwrap();
  f.signal("continue");
  assert!(capture.await.unwrap().is_err());
  assert!(remote::status(&f.manager, ID).await.is_err());
  f.sessions.stop(ID, Actor::Person).await.unwrap();
}

#[tokio::test]
async fn malformed_headers_and_boot_commands_cannot_reach_the_worker() {
  let f = Fixture::new();
  f.start("normal").await;
  let _server = f.sessions.serve_agents().unwrap();
  let endpoint = f.manager.root.join("agents/native.sock");
  for bytes in [
    [b"HPA1".as_slice(), &4097u32.to_le_bytes()].concat(),
    [b"HPV1".as_slice(), &1u32.to_le_bytes()].concat(),
    {
      let header = format!(r#"{{"vmId":"{ID}","operation":"start","boot":{{}}}}"#);
      [
        b"HPA1".as_slice(),
        &(header.len() as u32).to_le_bytes(),
        header.as_bytes(),
      ]
      .concat()
    },
  ] {
    let mut stream = UnixStream::connect(&endpoint).await.unwrap();
    stream.write_all(&bytes).await.unwrap();
    let mut received = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(2), stream.read_to_end(&mut received))
      .await
      .unwrap();
    assert!(received.is_empty());
  }
  assert!(f.trace().is_empty());
  assert!(remote::capture(&f.manager, ID).await.is_ok());
  f.sessions.stop(ID, Actor::Person).await.unwrap();
}

#[tokio::test]
async fn private_endpoint_guards_preserve_unrelated_files_and_replacements() {
  let f = Fixture::new();
  let server = f.sessions.serve_agents().unwrap();
  let endpoint = f.manager.root.join("agents/native.sock");
  std::fs::set_permissions(&endpoint, std::fs::Permissions::from_mode(0o666)).unwrap();
  assert!(remote::status(&f.manager, ID).await.is_err());
  std::fs::remove_file(&endpoint).unwrap();
  std::fs::write(&endpoint, b"unrelated replacement").unwrap();
  drop(server);
  assert_eq!(std::fs::read(&endpoint).unwrap(), b"unrelated replacement");
  assert!(f.sessions.serve_agents().is_err());
  std::fs::remove_file(&endpoint).unwrap();
  let target = f.root.path().join("protected");
  std::fs::write(&target, b"protected").unwrap();
  std::os::unix::fs::symlink(&target, &endpoint).unwrap();
  assert!(f.sessions.serve_agents().is_err());
  assert_eq!(std::fs::read(&target).unwrap(), b"protected");
}

#[tokio::test]
async fn disconnected_capture_is_drained_before_the_next_frame() {
  let f = Fixture::new();
  f.start("cancel").await;
  let _server = f.sessions.serve_agents().unwrap();
  let manager = f.manager.clone();
  let capture = tokio::spawn(async move { remote::capture(&manager, ID).await });
  f.marker("partial").await;
  capture.abort();
  let _ = capture.await;
  f.signal("continue");
  let frame = tokio::time::timeout(Duration::from_secs(3), remote::capture(&f.manager, ID))
    .await
    .unwrap()
    .unwrap();
  assert_eq!(frame.rgba, [1, 2, 3, 255]);
  f.sessions.stop(ID, Actor::Person).await.unwrap();
}

#[tokio::test]
async fn an_idle_agent_service_cannot_retain_vm_ownership() {
  let f = Fixture::new();
  f.start("normal").await;
  let server = f.sessions.serve_agents().unwrap();
  let lease = f.lock(".runtime");
  drop(f.sessions);
  fixture::wait_lock(&lease, false).await;
  assert!(remote::capture(&f.manager, ID).await.is_err());
  drop(server);
}

#[test]
fn service_start_without_an_async_runtime_returns_an_error() {
  let f = Fixture::new();
  assert!(f.sessions.serve_agents().is_err());
  assert!(!f.manager.root.join("agents").exists());
}
