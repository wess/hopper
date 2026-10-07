#[allow(dead_code)]
#[path = "../src/bridge.rs"]
mod bridge;
#[allow(dead_code)]
#[path = "../src/views/machines/native/input.rs"]
mod input;

#[test]
fn pointer_mapping_excludes_letterbox_margins_and_bounds_guest_coordinates() {
  let viewport = [10.0, 20.0, 1000.0, 500.0];
  let image = [1000.0, 1000.0];
  assert_eq!(
    input::position(viewport, image, [260.0, 20.0]),
    Some([0, 0])
  );
  assert_eq!(
    input::position(viewport, image, [760.0, 520.0]),
    Some([65535, 65535])
  );
  assert_eq!(
    input::position(viewport, image, [510.0, 270.0]),
    Some([32768, 32768])
  );
  assert!(input::position(viewport, image, [259.0, 270.0]).is_none());
  assert!(input::position(viewport, image, [761.0, 270.0]).is_none());
  assert!(input::position(viewport, [0.0, 1000.0], [510.0, 270.0]).is_none());
  assert!(input::position(viewport, image, [f32::NAN, 270.0]).is_none());
}

#[test]
fn keyboard_names_and_synchronized_packets_match_guest_event_codes() {
  use model::native::{Command, InputDevice};
  assert_eq!(input::code("enter"), Some(28));
  assert_eq!(input::code("!"), input::code("1"));
  assert_eq!(input::code("up"), Some(103));
  assert_eq!(input::code("é"), None);
  let Command::Input { device, events } =
    input::events(InputDevice::Keyboard, vec![input::key(30, 1)])
  else {
    panic!("Expected a keyboard packet");
  };
  assert!(matches!(device, InputDevice::Keyboard));
  assert_eq!((events[1].kind, events[1].code, events[1].value), (0, 0, 0));
}

#[cfg(unix)]
#[test]
fn focus_release_discards_queued_input_and_overflow_releases_before_viewer_drop() {
  use model::native::{Boot, InputDevice};
  use std::{os::unix::fs::PermissionsExt, time::Duration};
  let root = tempfile::tempdir().unwrap();
  let previous = std::env::var_os("HOPPER_DIR");
  std::env::set_var("HOPPER_DIR", root.path());
  let host = host::Host::from_env();
  match previous {
    Some(value) => std::env::set_var("HOPPER_DIR", value),
    None => std::env::remove_var("HOPPER_DIR"),
  }
  let id = "8197e0f0-0603-43e9-a817-eaf7ab0327af";
  let machine = model::Machine {
    id: id.into(),
    name: "Synthetic viewer input".into(),
    guest: model::GuestOs::Windows,
    profile: "windows".into(),
    resources: model::EngineResources {
      cpus: 2,
      memory_gib: 2,
      disk_gib: 64,
    },
    installer: None,
    agent_access: true,
  };
  store::json::write(
    &root
      .path()
      .join("machines/records")
      .join(format!("{id}.json")),
    &machine,
  )
  .unwrap();
  let guest = root.path().join("guest");
  std::fs::create_dir(&guest).unwrap();
  let helper = root.path().join("worker");
  std::fs::write(&helper, include_str!("../../engine/tests/native/worker.py")).unwrap();
  std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700)).unwrap();
  bridge::runtime().block_on(async {
    host
      .native_machines()
      .start(
        id,
        &helper,
        Boot {
          firmware: String::new(),
          variables: String::new(),
          store: guest.to_string_lossy().into(),
          boot_media: None,
          installer: None,
          disk: None,
          disk_id: "inputdelay".into(),
          memory: 2 * 1024 * 1024 * 1024,
          cpus: 2,
          timeout_ms: None,
        },
      )
      .await
      .unwrap();
    let rows = host
      .list_machines(host::MachineActor::Person)
      .await
      .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, "Running");
    assert!(!root.path().join("machines/lima").exists());
    let driver = input::create(host.clone(), id.into());
    assert!(driver.send(input::events(
      InputDevice::Keyboard,
      vec![input::key(30, 1)]
    )));
    tokio::time::timeout(Duration::from_secs(2), async {
      while !guest.join("inputreceived").exists() {
        tokio::time::sleep(Duration::from_millis(5)).await;
      }
    })
    .await
    .unwrap();
    for _ in 0..32 {
      assert!(driver.send(input::events(
        InputDevice::Keyboard,
        vec![input::key(31, 1)]
      )));
    }
    assert!(!driver.send(input::events(
      InputDevice::Keyboard,
      vec![input::key(32, 1)]
    )));
    driver.release();
    drop(driver);
    let reopened = input::create(host.clone(), id.into());
    assert!(reopened.send(input::events(
      InputDevice::Keyboard,
      vec![input::key(33, 1)]
    )));
    tokio::time::timeout(Duration::from_secs(2), async {
      while reopened.error.borrow().is_none() {
        tokio::time::sleep(Duration::from_millis(5)).await;
      }
    })
    .await
    .unwrap();
    assert!(reopened
      .error
      .borrow()
      .as_ref()
      .unwrap()
      .contains("controlled by another connection"));
    std::fs::write(guest.join("inputcontinue"), "").unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
      while !std::fs::read_to_string(guest.join("trace"))
        .unwrap()
        .lines()
        .any(|line| line == "release")
      {
        tokio::time::sleep(Duration::from_millis(5)).await;
      }
    })
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    let trace = std::fs::read_to_string(guest.join("trace")).unwrap();
    assert_eq!(trace.lines().filter(|line| *line == "input").count(), 1);
    assert!(reopened.send(input::events(
      InputDevice::Keyboard,
      vec![input::key(34, 1)]
    )));
    tokio::time::timeout(Duration::from_secs(2), async {
      while std::fs::read_to_string(guest.join("trace"))
        .unwrap()
        .lines()
        .filter(|line| *line == "input")
        .count()
        != 2
      {
        tokio::time::sleep(Duration::from_millis(5)).await;
      }
    })
    .await
    .unwrap();
    let connected = std::fs::read_to_string(guest.join("trace")).unwrap();
    assert!(connected.ends_with("input\n"));
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert_eq!(
      std::fs::read_to_string(guest.join("trace")).unwrap(),
      connected
    );
    reopened.release();
    drop(reopened);
    tokio::time::timeout(Duration::from_secs(2), async {
      while !std::fs::read_to_string(guest.join("trace"))
        .unwrap()
        .ends_with("release\n")
      {
        tokio::time::sleep(Duration::from_millis(5)).await;
      }
    })
    .await
    .unwrap();
    assert_eq!(
      host
        .native_machines()
        .state(id, host::MachineActor::Person)
        .await
        .unwrap(),
      Some(host::MachineState::Running)
    );
    host
      .native_machines()
      .stop(id, host::MachineActor::Person)
      .await
      .unwrap();
  });
}
