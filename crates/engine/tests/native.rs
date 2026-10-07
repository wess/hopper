#![cfg(unix)]

use engine::machines::native::{self, Client, State};
use model::native::{Boot, Command, Result as Reply, StopReason};
use std::{os::unix::fs::PermissionsExt, path::Path, process, time::Duration};
use tempfile::TempDir;

fn fixture(mode: &str) -> (TempDir, Boot) {
  let root = tempfile::tempdir().unwrap();
  let helper = root.path().join("worker");
  std::fs::write(&helper, include_str!("native/worker.py")).unwrap();
  std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700)).unwrap();
  let boot = Boot {
    firmware: String::new(),
    variables: String::new(),
    store: root.path().to_string_lossy().into(),
    boot_media: None,
    installer: None,
    disk: None,
    disk_id: mode.into(),
    memory: 0x10000000,
    cpus: 2,
    timeout_ms: None,
  };
  (root, boot)
}

async fn launch(mode: &str) -> (TempDir, Client) {
  let (root, boot) = fixture(mode);
  let client = native::launch(&root.path().join("worker"), boot)
    .await
    .unwrap();
  (root, client)
}

async fn wait_file(path: &Path) {
  tokio::time::timeout(Duration::from_secs(3), async {
    while !path.exists() {
      tokio::time::sleep(Duration::from_millis(5)).await;
    }
  })
  .await
  .unwrap();
}

async fn exited(root: &TempDir) {
  let pid = std::fs::read_to_string(root.path().join("pid")).unwrap();
  tokio::time::timeout(Duration::from_secs(3), async {
    loop {
      let exists = process::Command::new("kill")
        .args(["-0", pid.trim()])
        .stderr(process::Stdio::null())
        .status()
        .unwrap()
        .success();
      if !exists {
        break;
      }
      tokio::time::sleep(Duration::from_millis(10)).await;
    }
  })
  .await
  .expect("worker must be terminated and reaped");
}

async fn terminal(client: &mut Client) -> State {
  tokio::time::timeout(Duration::from_secs(3), async {
    loop {
      let state = native::state(client);
      if matches!(state, State::Failed(_) | State::Stopped(_)) {
        break state;
      }
      native::changed(client).await.unwrap();
    }
  })
  .await
  .unwrap()
}

#[tokio::test]
async fn fragmented_frames_and_pause_state_round_trip() {
  let (root, client) = launch("normal").await;
  let frame = native::capture(&client).await.unwrap();
  assert_eq!((frame.width, frame.height, frame.generation), (1, 1, 7));
  assert_eq!(frame.rgba, [1, 2, 3, 255]);
  native::request(&client, Command::Pause {}).await.unwrap();
  assert_eq!(native::state(&client), State::Paused);
  native::request(&client, Command::Resume {}).await.unwrap();
  assert_eq!(native::state(&client), State::Running);
  native::request(&client, Command::Stop {}).await.unwrap();
  assert_eq!(
    native::state(&client),
    State::Stopped(StopReason::Requested)
  );
  exited(&root).await;
}

#[tokio::test]
async fn cancellation_during_a_partial_frame_does_not_corrupt_the_next_reply() {
  let (root, client) = launch("cancel").await;
  let pending = client.clone();
  let task = tokio::spawn(async move { native::capture(&pending).await });
  wait_file(&root.path().join("partial")).await;
  task.abort();
  assert!(matches!(task.await, Err(error) if error.is_cancelled()));
  std::fs::write(root.path().join("continue"), []).unwrap();
  let reply = native::request(&client, Command::Status {}).await.unwrap();
  assert!(matches!(reply, Reply::Status { paused: false, .. }));
  native::request(&client, Command::Stop {}).await.unwrap();
  assert_eq!(
    std::fs::read_to_string(root.path().join("trace")).unwrap(),
    "capture\nstatus\nstop\n"
  );
  exited(&root).await;
}

#[tokio::test]
async fn invalid_replies_fail_the_session_and_reap_the_worker() {
  for mode in ["wrongid", "wrongtype", "oversized"] {
    let (root, mut client) = launch(mode).await;
    assert!(native::capture(&client).await.is_err(), "{mode}");
    assert!(
      matches!(terminal(&mut client).await, State::Failed(_)),
      "{mode}"
    );
    exited(&root).await;
    assert!(native::request(&client, Command::Status {}).await.is_err());
  }
}

#[tokio::test]
async fn guest_shutdown_and_unexpected_exit_update_the_session() {
  for mode in ["shutdown", "exit"] {
    let (root, mut client) = launch(mode).await;
    let state = terminal(&mut client).await;
    if mode == "shutdown" {
      assert_eq!(state, State::Stopped(StopReason::Shutdown));
    } else {
      assert!(matches!(state, State::Failed(_)));
    }
    exited(&root).await;
  }
}

#[tokio::test]
async fn dropping_the_last_client_stops_and_reaps_the_worker() {
  let (root, client) = launch("normal").await;
  let other = client.clone();
  drop(client);
  native::request(&other, Command::Release {}).await.unwrap();
  drop(other);
  exited(&root).await;
  assert_eq!(
    std::fs::read_to_string(root.path().join("trace")).unwrap(),
    "release\nstop\n"
  );
}

#[tokio::test]
async fn rejected_commands_do_not_poison_the_session() {
  let (root, client) = launch("reject").await;
  assert!(native::request(&client, Command::Release {})
    .await
    .unwrap_err()
    .to_string()
    .contains("fixture rejection"));
  assert!(matches!(
    native::request(&client, Command::Status {}).await.unwrap(),
    Reply::Status { .. }
  ));
  native::request(&client, Command::Stop {}).await.unwrap();
  exited(&root).await;
}

#[tokio::test]
async fn invalid_startup_reaps_the_worker() {
  for mode in ["startupreject", "startupwrong"] {
    let (root, boot) = fixture(mode);
    assert!(native::launch(&root.path().join("worker"), boot)
      .await
      .is_err());
    exited(&root).await;
  }
}

#[tokio::test]
async fn cancelled_startup_reaps_the_worker() {
  let (root, boot) = fixture("startuphang");
  let helper = root.path().join("worker");
  let task = tokio::spawn(async move { native::launch(&helper, boot).await });
  wait_file(&root.path().join("pid")).await;
  task.abort();
  assert!(matches!(task.await, Err(error) if error.is_cancelled()));
  exited(&root).await;
}
