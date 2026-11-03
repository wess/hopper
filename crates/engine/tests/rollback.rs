#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[allow(dead_code)]
#[path = "restore/fixture.rs"]
mod fixture;
use engine::machines::{
  native::{
    restore::{Owner, Prepared},
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

fn no_history(f: &Fixture) {
  let paths = engine::machines::native::config::paths(&f.manager.root, ID).unwrap();
  assert!(!paths
    .root
    .join(format!("restore{}.hrr", fixture::identity([4; 16])))
    .exists());
}

#[tokio::test]
async fn failed_admission_waits_for_actual_exit_and_preserves_unrecoverable_components() {
  let (f, boot, restore, owner) = fixture::worker("restoredwrongconfig");
  let root = std::path::PathBuf::from(&boot.store);
  fs::write(root.join("stopgate"), []).unwrap();
  let paths = engine::machines::native::config::paths(&f.manager.root, ID).unwrap();
  let original = fs::read(paths.root.join("restore.hrr")).unwrap();
  let sessions = Arc::new(Sessions::new(f.manager.clone()));
  let started = sessions.clone();
  let helper = f.helper.clone();
  let task = tokio::spawn(async move {
    started
      .start_recoverable(
        &helper,
        Prepared {
          owner,
          boot,
          restore,
        },
      )
      .await
  });
  gate(&root, "stopreceived").await;
  assert!(Owner::person(f.manager.clone(), ID).is_err());
  assert!(!task.is_finished());
  fs::write(root.join("stopcontinue"), []).unwrap();
  gate(&root, "stopacknowledged").await;
  assert!(Owner::person(f.manager.clone(), ID).is_err());
  assert!(!task.is_finished());
  fs::write(root.join("exitcontinue"), []).unwrap();
  let error = task.await.unwrap().unwrap_err();
  assert!(error
    .to_string()
    .contains("prior-state recovery did not complete"));
  assert!(format!("{error:#}").contains("hardware differs"));
  assert_eq!(fs::read(paths.root.join("restore.hrr")).unwrap(), original);
  no_history(&f);
  drop(Owner::person(f.manager.clone(), ID).unwrap());
  assert!(sessions.state(ID, Actor::Person).await.unwrap().is_none());
}

#[tokio::test]
async fn off_on_revocation_during_failed_admission_does_not_reauthorize_rollback() {
  let (f, boot, restore, owner) = fixture::worker("restoredstartupgate");
  let root = std::path::PathBuf::from(&boot.store);
  let sessions = Arc::new(Sessions::new(f.manager.clone()));
  let started = sessions.clone();
  let helper = f.helper.clone();
  let task = tokio::spawn(async move {
    started
      .start_recoverable(
        &helper,
        Prepared {
          owner,
          boot,
          restore,
        },
      )
      .await
  });
  gate(&root, "pid").await;
  f.manager.set_agent_access(ID, false).unwrap();
  f.manager.set_agent_access(ID, true).unwrap();
  fs::write(root.join("startupcontinue"), []).unwrap();
  let error = task.await.unwrap().unwrap_err();
  assert!(error.to_string().contains("recovery did not complete"));
  no_history(&f);
  assert_eq!(
    fs::read_to_string(root.join("boots"))
      .unwrap()
      .lines()
      .count(),
    1
  );
  assert!(sessions.state(ID, Actor::Person).await.unwrap().is_none());
}

#[tokio::test]
async fn cancelled_admission_does_not_start_an_automatic_recovery() {
  let (f, boot, restore, owner) = fixture::worker("restoredstartupgate");
  let root = std::path::PathBuf::from(&boot.store);
  let sessions = Arc::new(Sessions::new(f.manager.clone()));
  let started = sessions.clone();
  let helper = f.helper.clone();
  let task = tokio::spawn(async move {
    started
      .start_recoverable(
        &helper,
        Prepared {
          owner,
          boot,
          restore,
        },
      )
      .await
  });
  gate(&root, "pid").await;
  task.abort();
  assert!(task.await.unwrap_err().is_cancelled());
  tokio::time::timeout(Duration::from_secs(4), async {
    while Owner::person(f.manager.clone(), ID).is_err() {
      tokio::time::sleep(Duration::from_millis(5)).await;
    }
  })
  .await
  .unwrap();
  no_history(&f);
  assert_eq!(
    fs::read_to_string(root.join("boots"))
      .unwrap()
      .lines()
      .count(),
    1
  );
  assert!(sessions.state(ID, Actor::Person).await.unwrap().is_none());
}

#[tokio::test]
async fn foreign_prepared_receipt_rejects_without_startup_or_rollback() {
  let (f, boot, mut restore, owner) = fixture::worker("restored");
  let root = std::path::PathBuf::from(&boot.store);
  restore.snapshot_id = "09090909-0909-0909-0909-090909090909".into();
  let sessions = Sessions::new(f.manager.clone());
  assert!(sessions
    .start_recoverable(
      &f.helper,
      Prepared {
        owner,
        boot,
        restore
      }
    )
    .await
    .is_err());
  assert!(!root.join("pid").exists());
  no_history(&f);
  drop(Owner::person(f.manager.clone(), ID).unwrap());
}

#[tokio::test]
async fn changed_selection_during_failed_worker_cleanup_rejects_recovery_without_replacement() {
  use std::os::unix::fs::PermissionsExt;
  let (f, boot, restore, owner) = fixture::worker("restoredwrongconfig");
  let root = std::path::PathBuf::from(&boot.store);
  fs::write(root.join("stopgate"), []).unwrap();
  let paths = engine::machines::native::config::paths(&f.manager.root, ID).unwrap();
  let mut foreign = owner.selected().unwrap().unwrap();
  foreign.snapshot = [9; 16];
  let bytes = foreign.encode().unwrap();
  let snapshots = fs::read_dir(paths.root.join("snapshots")).unwrap().count();
  let sessions = Arc::new(Sessions::new(f.manager.clone()));
  let started = sessions.clone();
  let helper = f.helper.clone();
  let task = tokio::spawn(async move {
    started
      .start_recoverable(
        &helper,
        Prepared {
          owner,
          boot,
          restore,
        },
      )
      .await
  });
  gate(&root, "stopreceived").await;
  let selection = paths.root.join("restore.hrr");
  fs::set_permissions(&selection, fs::Permissions::from_mode(0o600)).unwrap();
  fs::write(&selection, &bytes).unwrap();
  fs::set_permissions(&selection, fs::Permissions::from_mode(0o400)).unwrap();
  fs::write(root.join("stopcontinue"), []).unwrap();
  gate(&root, "stopacknowledged").await;
  fs::write(root.join("exitcontinue"), []).unwrap();
  let error = task.await.unwrap().unwrap_err();
  assert!(format!("{error:#}").contains("selection changed before recovery"));
  assert_eq!(fs::read(selection).unwrap(), bytes);
  assert_eq!(
    fs::read_dir(paths.root.join("snapshots")).unwrap().count(),
    snapshots
  );
  assert_eq!(
    fs::read_to_string(root.join("boots"))
      .unwrap()
      .lines()
      .count(),
    1
  );
  no_history(&f);
  drop(Owner::person(f.manager.clone(), ID).unwrap());
  assert!(sessions.state(ID, Actor::Person).await.unwrap().is_none());
}
