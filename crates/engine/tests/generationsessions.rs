#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[allow(dead_code)]
#[path = "restore/fixture.rs"]
mod fixture;
use engine::machines::{
  native::{
    self,
    restore::{Owner, Phase},
    sessions::Sessions,
    State,
  },
  Actor,
};
use fixture::{Fixture, ID};
use model::native::{Boot, Command, Restore};
use std::{
  fs,
  path::{Path, PathBuf},
  sync::Arc,
  time::Duration,
};

async fn until(mut ready: impl FnMut() -> bool) {
  tokio::time::timeout(Duration::from_secs(4), async {
    while !ready() {
      tokio::time::sleep(Duration::from_millis(5)).await;
    }
  })
  .await
  .expect("generation worker gate timed out");
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
async fn released(f: &Fixture, root: &Path) {
  until(|| !alive(root) && Owner::person(f.manager.clone(), ID).is_ok()).await;
}
async fn active(mode: &str) -> (Fixture, Boot, Restore, Owner) {
  let (f, mut boot, restore, owned) = fixture::worker("restored");
  let root = PathBuf::from(&boot.store);
  let owned = Arc::new(owned);
  let weak = Arc::downgrade(&owned);
  let client = native::launch_restored(&f.helper, boot.clone(), restore.clone(), owned.clone())
    .await
    .unwrap();
  native::resume_restored(&client, restore.clone(), owned.clone())
    .await
    .unwrap();
  native::request(&client, Command::Stop {}).await.unwrap();
  native::finished(&client).await.unwrap();
  drop(client);
  drop(owned);
  until(|| weak.upgrade().is_none()).await;
  fs::remove_file(root.join("stopacknowledged")).unwrap();
  boot.disk_id = mode.into();
  let owned = Owner::agent(f.manager.clone(), ID, 0).unwrap();
  assert_eq!(owned.selected().unwrap().unwrap().phase, Phase::Active);
  (f, boot, restore, owned)
}

#[tokio::test]
async fn cold_session_serves_viewer_operations_and_preserves_active_receipt() {
  let (f, boot, restore, owned) = active("generation").await;
  let root = PathBuf::from(&boot.store);
  let receipt = root.parent().unwrap().join("restore.hrr");
  let bytes = fs::read(&receipt).unwrap();
  let before = fs::metadata(&receipt).unwrap().modified().unwrap();
  let sessions = Sessions::new(f.manager.clone());
  sessions
    .start_generation(ID, &f.helper, boot, restore, owned)
    .await
    .unwrap();
  let watch = sessions.watch(ID).await.unwrap();
  assert!(matches!(*watch.borrow(), State::Running));
  assert_eq!(
    sessions.capture(ID, Actor::Agent).await.unwrap().rgba,
    [1, 2, 3, 255]
  );
  sessions
    .request(ID, Actor::Person, Command::Pause {})
    .await
    .unwrap();
  sessions
    .request(ID, Actor::Person, Command::Resume {})
    .await
    .unwrap();
  assert!(Owner::person(f.manager.clone(), ID).is_err());
  f.manager.set_agent_access(ID, false).unwrap();
  assert!(sessions.capture(ID, Actor::Agent).await.is_err());
  sessions.stop(ID, Actor::Person).await.unwrap();
  assert!(!alive(&root));
  drop(Owner::person(f.manager.clone(), ID).unwrap());
  assert_eq!(fs::read(&receipt).unwrap(), bytes);
  assert_eq!(fs::metadata(&receipt).unwrap().modified().unwrap(), before);
}

#[tokio::test]
async fn cold_start_cancellation_and_revocation_reap_without_publication() {
  for revoke in [false, true] {
    let (f, boot, restore, owned) = active("generationstartupgate").await;
    let root = PathBuf::from(&boot.store);
    let sessions = Arc::new(Sessions::new(f.manager.clone()));
    let started = sessions.clone();
    let helper = f.helper.clone();
    let task = tokio::spawn(async move {
      started
        .start_generation(ID, &helper, boot, restore, owned)
        .await
    });
    gate(&root, "generationreceived").await;
    if revoke {
      f.manager.set_agent_access(ID, false).unwrap();
      assert!(task.await.unwrap().is_err());
    } else {
      task.abort();
      assert!(task.await.unwrap_err().is_cancelled());
    }
    released(&f, &root).await;
    assert!(sessions.state(ID, Actor::Person).await.unwrap().is_none());
  }
}

#[tokio::test]
async fn cold_resume_cancellation_drains_before_releasing_runtime() {
  let (f, boot, restore, owned) = active("generationresumedelay").await;
  let root = PathBuf::from(&boot.store);
  let sessions = Arc::new(Sessions::new(f.manager.clone()));
  let started = sessions.clone();
  let helper = f.helper.clone();
  let task = tokio::spawn(async move {
    started
      .start_generation(ID, &helper, boot, restore, owned)
      .await
  });
  gate(&root, "resumereceived").await;
  assert!(f.manager.set_agent_access(ID, false).is_err());
  task.abort();
  assert!(task.await.unwrap_err().is_cancelled());
  fs::write(root.join("resumecontinue"), []).unwrap();
  gate(&root, "stopreceived").await;
  assert!(Owner::person(f.manager.clone(), ID).is_err());
  fs::write(root.join("stopcontinue"), []).unwrap();
  gate(&root, "stopacknowledged").await;
  assert!(alive(&root));
  assert!(Owner::person(f.manager.clone(), ID).is_err());
  fs::write(root.join("exitcontinue"), []).unwrap();
  released(&f, &root).await;
  assert!(sessions.state(ID, Actor::Person).await.unwrap().is_none());
}

#[tokio::test]
async fn cold_start_rejects_configuration_and_boot_kind_substitution() {
  for mode in ["generationwrongconfig", "generationwrongreply"] {
    let (f, boot, restore, owned) = active(mode).await;
    let root = PathBuf::from(&boot.store);
    let sessions = Sessions::new(f.manager.clone());
    assert!(sessions
      .start_generation(ID, &f.helper, boot, restore, owned)
      .await
      .is_err());
    released(&f, &root).await;
    assert!(sessions.state(ID, Actor::Person).await.unwrap().is_none());
  }
}

#[tokio::test]
async fn cold_start_never_executes_an_attempted_checkpoint_generation() {
  let (f, boot, restore, owned) = fixture::worker("generation");
  let root = PathBuf::from(&boot.store);
  let sessions = Sessions::new(f.manager.clone());
  assert!(sessions
    .start_generation(ID, &f.helper, boot, restore, owned)
    .await
    .is_err());
  assert!(!root.join("pid").exists());
  assert_eq!(
    Owner::person(f.manager.clone(), ID)
      .unwrap()
      .selected()
      .unwrap()
      .unwrap()
      .phase,
    Phase::Attempted
  );
}
