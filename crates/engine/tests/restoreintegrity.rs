#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "snapshotcontroller/fixture.rs"]
mod fixture;
use engine::machines::{native::State, Actor};
use fixture::{Fixture, ID};
use std::{fs, os::unix::fs::PermissionsExt};
const SOURCE: &str = "02020202-0202-0202-0202-020202020202";

#[tokio::test]
async fn damaged_components_reject_before_pause_capture_or_stop_and_preserve_current_vm() {
  for fault in 0..4 {
    let f = Fixture::new("snapshotgates").await;
    f.ready(SOURCE);
    f.marker("pollcontinue");
    f.marker("cancelcontinue");
    f.source
      .sessions
      .snapshot(ID, Actor::Person, SOURCE)
      .await
      .unwrap();
    f.wait("snapshotpoll").await;
    let before = f.trace();
    let path = f
      .root
      .join("snapshots")
      .join(SOURCE.replace('-', ""))
      .join("ram.hrm");
    let original = fs::read(&path).unwrap();
    let mut damaged = original.clone();
    match fault {
      0 => damaged[0] ^= 1,
      1 => {
        damaged.pop();
      }
      2 => damaged.push(0),
      _ => damaged[original.len() - 1] ^= 1,
    }
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(&path, &damaged).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
    let failure = f
      .source
      .sessions
      .restore_snapshot(ID, Actor::Agent, SOURCE)
      .await
      .err()
      .unwrap();
    assert!(format!("{failure:#}").contains("component"), "{failure:#}");
    f.released().await;
    assert!(!f.locked());
    assert_eq!(f.trace(), before);
    assert_eq!(
      f.source.sessions.state(ID, Actor::Person).await.unwrap(),
      Some(State::Running)
    );
    assert_eq!(fs::read(path).unwrap(), damaged);
    assert_eq!(fs::read_dir(f.root.join("snapshots")).unwrap().count(), 1);
    assert!(!f.root.join("restore.hrr").exists());
    assert!(!f.root.join("generations").exists());
    f.source.sessions.stop(ID, Actor::Person).await.unwrap();
  }
}
