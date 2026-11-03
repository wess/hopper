#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[allow(dead_code)]
#[path = "restore/fixture.rs"]
mod fixture;
use engine::machines::{
  native::{
    restore::{Owner, Phase},
    sessions::Sessions,
  },
  Actor,
};
use fixture::{Fixture, ID};
use std::{fs, path::Path, sync::Arc, time::Duration};

async fn gate(root: &Path, name: &str) {
  tokio::time::timeout(Duration::from_secs(4), async {
    while !root.join(name).exists() {
      tokio::time::sleep(Duration::from_millis(5)).await;
    }
  })
  .await
  .unwrap();
}

fn held(f: &Fixture) {
  assert!(Owner::person(f.manager.clone(), ID).is_err());
}

async fn released(f: &Fixture) {
  tokio::time::timeout(Duration::from_secs(4), async {
    while Owner::person(f.manager.clone(), ID).is_err() {
      tokio::time::sleep(Duration::from_millis(5)).await;
    }
  })
  .await
  .unwrap();
}

fn active(f: &Fixture, root: &Path) {
  let selected = engine::machines::native::restore::read(
    &fs::File::open(root.parent().unwrap()).unwrap(),
    model::native::machine_identity(ID).unwrap(),
  )
  .unwrap()
  .unwrap();
  assert_eq!(selected.phase, Phase::Active);
  assert_eq!(selected.generation, [4; 16]);
  assert_eq!(selected.snapshot, [2; 16]);
  let trace = fs::read_to_string(root.join("trace")).unwrap();
  assert_eq!(trace, "restorePause\nstop\n");
  let pid = fs::read_to_string(root.join("pid"))
    .unwrap()
    .parse()
    .unwrap();
  assert_ne!(unsafe { libc::kill(pid, 0) }, 0);
  drop(Owner::person(f.manager.clone(), ID).unwrap());
}

#[tokio::test]
async fn stopped_activation_keeps_all_leases_and_hides_session_until_actual_exit() {
  let (f, boot, restore, owned) = fixture::worker("restoredstopdelay");
  let root = std::path::PathBuf::from(&boot.store);
  let sessions = Arc::new(Sessions::new(f.manager.clone()));
  let started = sessions.clone();
  let helper = f.helper.clone();
  let task = tokio::spawn(async move {
    started
      .start_restored_stopped(ID, &helper, boot, restore, owned)
      .await
  });
  gate(&root, "stopreceived").await;
  held(&f);
  assert!(!task.is_finished());
  let inspected = sessions.clone();
  let status = tokio::spawn(async move { inspected.state(ID, Actor::Person).await });
  assert!(!status.is_finished());
  fs::write(root.join("stopcontinue"), []).unwrap();
  gate(&root, "stopacknowledged").await;
  held(&f);
  assert!(!task.is_finished() && !status.is_finished());
  fs::write(root.join("exitcontinue"), []).unwrap();
  task.await.unwrap().unwrap();
  assert!(status.await.unwrap().unwrap().is_none());
  released(&f).await;
  active(&f, &root);
  assert!(sessions.watch(ID).await.is_err());
}

#[tokio::test]
async fn cancelled_stopped_activation_reaps_without_exposing_or_resuming_its_worker() {
  let (f, boot, restore, owned) = fixture::worker("restoredstopdelay");
  let root = std::path::PathBuf::from(&boot.store);
  let sessions = Arc::new(Sessions::new(f.manager.clone()));
  let started = sessions.clone();
  let helper = f.helper.clone();
  let task = tokio::spawn(async move {
    started
      .start_restored_stopped(ID, &helper, boot, restore, owned)
      .await
  });
  gate(&root, "stopreceived").await;
  task.abort();
  assert!(task.await.unwrap_err().is_cancelled());
  held(&f);
  fs::write(root.join("stopcontinue"), []).unwrap();
  gate(&root, "stopacknowledged").await;
  held(&f);
  fs::write(root.join("exitcontinue"), []).unwrap();
  released(&f).await;
  active(&f, &root);
  assert!(sessions.state(ID, Actor::Person).await.unwrap().is_none());
}

#[tokio::test]
async fn off_on_revocation_during_stopped_cleanup_does_not_reauthorize_completion() {
  let (f, boot, restore, owned) = fixture::worker("restoredstopdelay");
  let root = std::path::PathBuf::from(&boot.store);
  let sessions = Arc::new(Sessions::new(f.manager.clone()));
  let started = sessions.clone();
  let helper = f.helper.clone();
  let task = tokio::spawn(async move {
    started
      .start_restored_stopped(ID, &helper, boot, restore, owned)
      .await
  });
  gate(&root, "stopreceived").await;
  f.manager.set_agent_access(ID, false).unwrap();
  f.manager.set_agent_access(ID, true).unwrap();
  held(&f);
  fs::write(root.join("stopcontinue"), []).unwrap();
  gate(&root, "stopacknowledged").await;
  held(&f);
  fs::write(root.join("exitcontinue"), []).unwrap();
  let error = task.await.unwrap().unwrap_err();
  assert!(format!("{error:#}").contains("policy"), "{error:#}");
  released(&f).await;
  active(&f, &root);
  assert!(sessions.state(ID, Actor::Person).await.unwrap().is_none());
}
