#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[allow(dead_code)]
#[path = "restore/fixture.rs"]
mod fixture;
use engine::machines::{
  native::{self, restore::Owner, sessions::Sessions, State},
  Actor,
};
use fixture::{Fixture, ID};
use model::native::{Command, Result as Reply};
use std::{
  fs,
  path::{Path, PathBuf},
  time::Duration,
};

async fn until(mut ready: impl FnMut() -> bool) {
  tokio::time::timeout(Duration::from_secs(4), async {
    while !ready() {
      tokio::time::sleep(Duration::from_millis(5)).await;
    }
  })
  .await
  .expect("restored session gate timed out");
}
async fn gate(root: &Path, name: &str) {
  until(|| root.join(name).exists()).await;
}
fn alive(root: &Path) -> bool {
  let pid = fs::read_to_string(root.join("pid"))
    .unwrap()
    .parse()
    .unwrap();
  unsafe { libc::kill(pid, 0) == 0 }
}
fn held(f: &Fixture, root: &Path) {
  assert!(alive(root));
  assert!(Owner::person(f.manager.clone(), ID).is_err());
}
async fn released(f: &Fixture, root: &Path) {
  until(|| !alive(root) && Owner::person(f.manager.clone(), ID).is_ok()).await;
}

#[tokio::test]
async fn restored_session_serves_viewer_and_guest_operations_then_releases_runtime() {
  let (f, boot, restore, owned) = fixture::worker("restored");
  let root = PathBuf::from(&boot.store);
  let sessions = Sessions::new(f.manager.clone());
  sessions
    .start_restored(ID, &f.helper, boot, restore, owned)
    .await
    .unwrap();
  let watch = sessions.watch(ID).await.unwrap();
  assert!(matches!(*watch.borrow(), State::Running));
  held(&f, &root);
  // runtime exclusion remains, while ordinary operations can acquire their own lease.
  assert_eq!(
    sessions.capture(ID, Actor::Agent).await.unwrap().rgba,
    [1, 2, 3, 255]
  );
  assert!(matches!(
    sessions
      .request(ID, Actor::Agent, Command::Pause {})
      .await
      .unwrap(),
    Reply::Paused {}
  ));
  assert!(matches!(
    sessions
      .request(ID, Actor::Person, Command::Resume {})
      .await
      .unwrap(),
    Reply::Running {}
  ));
  f.manager.set_agent_access(ID, false).unwrap();
  assert!(sessions.capture(ID, Actor::Agent).await.is_err());
  sessions.stop(ID, Actor::Person).await.unwrap();
  assert!(!alive(&root));
  let reopened = Owner::person(f.manager.clone(), ID).unwrap();
  assert_eq!(
    reopened.selected().unwrap().unwrap().phase,
    native::restore::Phase::Active
  );
  drop(reopened);
  assert!(matches!(*watch.borrow(), State::Stopped(_)));
  assert!(native::config::initialize(&f.manager, ID).is_err());
}

#[tokio::test]
async fn paused_restore_activates_without_resume_and_keeps_person_control_after_revocation() {
  let (f, boot, restore, owned) = fixture::worker("restored");
  let root = PathBuf::from(&boot.store);
  let sessions = Sessions::new(f.manager.clone());
  sessions
    .start_restored_paused(ID, &f.helper, boot, restore, owned)
    .await
    .unwrap();
  let watch = sessions.watch(ID).await.unwrap();
  assert!(matches!(*watch.borrow(), State::Paused));
  assert_eq!(
    fs::read_to_string(root.join("trace")).unwrap(),
    "restorePause\n"
  );
  held(&f, &root);
  assert!(matches!(
    sessions
      .request(ID, Actor::Agent, Command::Status {})
      .await
      .unwrap(),
    Reply::Status { paused: true, .. }
  ));
  f.manager.set_agent_access(ID, false).unwrap();
  f.manager.set_agent_access(ID, true).unwrap();
  assert!(matches!(*watch.borrow(), State::Paused));
  sessions
    .request(ID, Actor::Person, Command::Resume {})
    .await
    .unwrap();
  assert!(matches!(*watch.borrow(), State::Running));
  sessions.stop(ID, Actor::Person).await.unwrap();
  assert!(!alive(&root));
  let selected = Owner::person(f.manager.clone(), ID)
    .unwrap()
    .selected()
    .unwrap()
    .unwrap();
  assert_eq!(selected.phase, native::restore::Phase::Active);
}

#[tokio::test]
async fn restored_registry_teardown_retains_runtime_past_stop_ack() {
  let (f, boot, restore, owned) = fixture::worker("restoredstopdelay");
  let root = PathBuf::from(&boot.store);
  let sessions = Sessions::new(f.manager.clone());
  sessions
    .start_restored(ID, &f.helper, boot, restore, owned)
    .await
    .unwrap();
  let watch = sessions.watch(ID).await.unwrap();
  drop(sessions);
  gate(&root, "stopreceived").await;
  held(&f, &root);
  fs::write(root.join("stopcontinue"), []).unwrap();
  gate(&root, "stopacknowledged").await;
  held(&f, &root);
  fs::write(root.join("exitcontinue"), []).unwrap();
  released(&f, &root).await;
  drop(watch);
}

#[tokio::test]
async fn cancelled_registry_startup_cleans_up_without_publishing() {
  let (f, boot, restore, owned) = fixture::worker("restoredstartupgate");
  let root = PathBuf::from(&boot.store);
  let sessions = std::sync::Arc::new(Sessions::new(f.manager.clone()));
  let started = sessions.clone();
  let helper = f.helper.clone();
  let task = tokio::spawn(async move {
    started
      .start_restored(ID, &helper, boot, restore, owned)
      .await
  });
  gate(&root, "pid").await;
  task.abort();
  assert!(task.await.unwrap_err().is_cancelled());
  released(&f, &root).await;
  assert!(sessions.state(ID, Actor::Person).await.unwrap().is_none());
}

#[tokio::test]
async fn cancelled_registry_resume_drains_and_reaps_without_publishing() {
  for paused in [false, true] {
    let (f, boot, restore, owned) = fixture::worker("restoredresumedelay");
    let root = PathBuf::from(&boot.store);
    let sessions = std::sync::Arc::new(Sessions::new(f.manager.clone()));
    let started = sessions.clone();
    let helper = f.helper.clone();
    let task = tokio::spawn(async move {
      if paused {
        started
          .start_restored_paused(ID, &helper, boot, restore, owned)
          .await
      } else {
        started
          .start_restored(ID, &helper, boot, restore, owned)
          .await
      }
    });
    gate(&root, "resumereceived").await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    fs::write(root.join("resumecontinue"), []).unwrap();
    gate(&root, "stopreceived").await;
    held(&f, &root);
    fs::write(root.join("stopcontinue"), []).unwrap();
    gate(&root, "stopacknowledged").await;
    held(&f, &root);
    fs::write(root.join("exitcontinue"), []).unwrap();
    released(&f, &root).await;
    assert!(sessions.state(ID, Actor::Person).await.unwrap().is_none());
  }
}

#[tokio::test]
async fn foreign_registry_owner_is_rejected_before_spawning() {
  let (f, boot, restore, owned) = fixture::worker("restored");
  let other = Fixture::new();
  let root = PathBuf::from(&boot.store);
  let sessions = Sessions::new(other.manager.clone());
  assert!(sessions
    .start_restored(ID, &f.helper, boot, restore, owned)
    .await
    .is_err());
  assert!(!root.join("pid").exists());
  drop(Owner::person(f.manager.clone(), ID).unwrap());
}

#[tokio::test]
async fn replaced_vm_namespace_is_rejected_before_spawning() {
  let (f, boot, restore, owned) = fixture::worker("restored");
  let root = PathBuf::from(&boot.store);
  let paths = native::config::paths(&f.manager.root, ID).unwrap();
  let moved = paths.root.with_extension("saved");
  fs::rename(&paths.root, &moved).unwrap();
  fixture::private(&paths.root);
  let sessions = Sessions::new(f.manager.clone());
  assert!(sessions
    .start_restored(ID, &f.helper, boot, restore, owned)
    .await
    .is_err());
  assert!(!root.join("pid").exists());
  assert!(!moved.join("variables/pid").exists());
}
