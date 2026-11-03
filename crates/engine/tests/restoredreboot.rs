#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[allow(dead_code)]
#[path = "restore/fixture.rs"]
mod fixture;
use engine::machines::{
  native::{restore::Owner, sessions::Sessions, State},
  Actor,
};
use fixture::{Fixture, ID};
use model::native::Command;
use std::{fs, path::Path, time::Duration};

async fn until(mut ready: impl FnMut() -> bool) {
  tokio::time::timeout(Duration::from_secs(4), async {
    while !ready() {
      tokio::time::sleep(Duration::from_millis(5)).await;
    }
  })
  .await
  .unwrap();
}
fn count(root: &Path) -> usize {
  fs::read_to_string(root.join("boots"))
    .unwrap()
    .lines()
    .count()
}
async fn reset(f: &Fixture, root: &Path, sessions: &Sessions) {
  fs::write(root.join("powerevent"), "reset").unwrap();
  let _ = sessions
    .request(ID, Actor::Person, Command::Status {})
    .await;
  assert!(f.manager.machine(ID, Actor::Person).is_ok());
}

#[tokio::test]
async fn restored_agent_handoff_reboots_fresh_without_checkpoint_replay() {
  let (f, boot, restore, owned) = fixture::worker("restored");
  let root = std::path::PathBuf::from(&boot.store);
  let sessions = Sessions::new(f.manager.clone());
  sessions
    .start_restored(ID, &f.helper, boot, restore, owned)
    .await
    .unwrap();
  let receipt = fs::read(root.parent().unwrap().join("restore.hrr")).unwrap();
  let watch = sessions.watch(ID).await.unwrap();
  for expected in [2, 3] {
    reset(&f, &root, &sessions).await;
    until(|| count(&root) == expected && matches!(*watch.borrow(), State::Running)).await;
    assert!(root.join("generationreceived").exists());
    assert_eq!(
      fs::read(root.parent().unwrap().join("restore.hrr")).unwrap(),
      receipt
    );
  }
  // the raw prepared handoff does not manufacture a deployment receipt.
  assert!(!root.parent().unwrap().join("installation").exists());
  sessions.stop(ID, Actor::Person).await.unwrap();
  drop(Owner::person(f.manager.clone(), ID).unwrap());
}

#[tokio::test]
async fn restored_agent_reboot_does_not_reauthorize_after_off_on() {
  let (f, boot, restore, owned) = fixture::worker("restored");
  let root = std::path::PathBuf::from(&boot.store);
  let sessions = Sessions::new(f.manager.clone());
  sessions
    .start_restored(ID, &f.helper, boot, restore, owned)
    .await
    .unwrap();
  let receipt = fs::read(root.parent().unwrap().join("restore.hrr")).unwrap();
  fs::write(root.join("generationgate"), []).unwrap();
  reset(&f, &root, &sessions).await;
  until(|| count(&root) == 2).await;
  f.manager.set_agent_access(ID, false).unwrap();
  f.manager.set_agent_access(ID, true).unwrap();
  until(|| Owner::person(f.manager.clone(), ID).is_ok()).await;
  fs::remove_file(root.join("generationgate")).unwrap();
  tokio::time::sleep(Duration::from_millis(250)).await;
  assert_eq!(count(&root), 2);
  assert!(!matches!(
    *sessions.watch(ID).await.unwrap().borrow(),
    State::Running
  ));
  assert_eq!(
    fs::read(root.parent().unwrap().join("restore.hrr")).unwrap(),
    receipt
  );
}

#[tokio::test]
async fn restored_reboot_cannot_retain_installer_media_or_fall_back() {
  let (f, mut boot, restore, owned) = fixture::worker("restored");
  let root = std::path::PathBuf::from(&boot.store);
  boot.boot_media = Some(f.root.path().join("setup.iso").to_string_lossy().into());
  let sessions = Sessions::new(f.manager.clone());
  sessions
    .start_restored(ID, &f.helper, boot, restore, owned)
    .await
    .unwrap();
  reset(&f, &root, &sessions).await;
  until(|| Owner::person(f.manager.clone(), ID).is_ok()).await;
  tokio::time::sleep(Duration::from_millis(250)).await;
  assert_eq!(count(&root), 1);
  assert!(!root.join("generationreceived").exists());
}
