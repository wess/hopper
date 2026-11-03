#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[allow(dead_code)]
#[path = "restore/fixture.rs"]
mod fixture;
use engine::machines::{
  native::{config, restore::Phase, sessions::Sessions},
  Actor,
};
use fixture::{Fixture, ID};
use fs2::FileExt;
use model::native::Command;
use std::{
  fs,
  os::unix::fs::{symlink, PermissionsExt},
};

fn selection(f: &Fixture, phase: Phase) {
  let mut record = fixture::record();
  record.phase = phase;
  let path = config::paths(&f.manager.root, ID)
    .unwrap()
    .root
    .join("restore.hrr");
  fs::write(&path, record.encode().unwrap()).unwrap();
  fs::set_permissions(path, fs::Permissions::from_mode(0o400)).unwrap();
}

fn installed(f: &Fixture) {
  let path = config::paths(&f.manager.root, ID)
    .unwrap()
    .root
    .join("installation");
  fs::write(&path, br#"{"version":1,"vmId":"8197e0f0-0603-43e9-a817-eaf7ab0327af","installation":{"state":"systemStarted"}}"#).unwrap();
  fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

#[tokio::test]
async fn pending_restore_is_recovery_even_without_installation_receipt() {
  for phase in [Phase::Prepared, Phase::Attempted] {
    let f = Fixture::new();
    f.initialize();
    selection(&f, phase);
    let sessions = Sessions::new(f.manager.clone());
    let rows = sessions.list_windows(Actor::Person).await.unwrap();
    assert_eq!(rows[0].state, "Recovery required");
    assert!(rows[0].progress.as_ref().unwrap().contains("interrupted"));
    assert!(!rows[0].busy);
  }
}

#[tokio::test]
async fn active_selection_reports_intent_without_claiming_guest_readiness() {
  let f = Fixture::new();
  f.initialize();
  selection(&f, Phase::Active);
  let sessions = Sessions::new(f.manager.clone());
  assert_eq!(
    sessions.list_windows(Actor::Person).await.unwrap()[0].state,
    "Recovery required"
  );
  installed(&f);
  let rows = sessions.list_windows(Actor::Person).await.unwrap();
  assert_eq!(rows[0].state, "Stopped");
  assert_eq!(
    rows[0].progress.as_deref(),
    Some("Restored state is selected")
  );
  // These metadata-only fixtures deliberately contain no usable generation or checkpoint.
}

#[tokio::test]
async fn unsafe_restore_metadata_is_preserved_and_does_not_hide_other_rows() {
  for linked in [false, true] {
    let f = Fixture::new();
    let paths = f.initialize();
    let record = paths.root.join("restore.hrr");
    let bytes = b"retained invalid restore metadata";
    if linked {
      let target = f.root.path().join("retained");
      fs::write(&target, bytes).unwrap();
      symlink(target, &record).unwrap();
    } else {
      fs::write(&record, bytes).unwrap();
      fs::set_permissions(&record, fs::Permissions::from_mode(0o400)).unwrap();
    }
    let mut other: model::Machine = serde_json::from_slice(&fs::read(f.record()).unwrap()).unwrap();
    other.id = "9197e0f0-0603-43e9-a817-eaf7ab0327af".into();
    other.name = "Other VM".into();
    store::json::write(
      &f.manager
        .root
        .join("records")
        .join(format!("{}.json", other.id)),
      &other,
    )
    .unwrap();
    let sessions = Sessions::new(f.manager.clone());
    let rows = sessions.list_windows(Actor::Person).await.unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
      rows.iter().find(|row| row.machine.id == ID).unwrap().state,
      "Recovery required"
    );
    assert_eq!(
      rows
        .iter()
        .find(|row| row.machine.id == other.id)
        .unwrap()
        .state,
      "Not created"
    );
    assert_eq!(fs::read(&record).unwrap(), bytes);
    assert_eq!(
      fs::symlink_metadata(record)
        .unwrap()
        .file_type()
        .is_symlink(),
      linked
    );
  }
}

#[tokio::test]
async fn busy_and_foreign_owners_keep_their_authoritative_status() {
  let (f, _, _, owner) = fixture::worker("restored");
  let sessions = Sessions::new(f.manager.clone());
  let rows = sessions.list_windows(Actor::Person).await.unwrap();
  assert!(rows[0].busy);
  assert_eq!(rows[0].state, "Not created");
  drop(owner);
  let runtime = fs::OpenOptions::new()
    .read(true)
    .write(true)
    .open(f.manager.root.join("locks").join(format!("{ID}.runtime")))
    .unwrap();
  runtime.try_lock_exclusive().unwrap();
  let rows = sessions.list_windows(Actor::Person).await.unwrap();
  assert!(rows[0].busy);
  assert_eq!(rows[0].state, "Running elsewhere");
  FileExt::unlock(&runtime).unwrap();
}

#[tokio::test]
async fn running_and_paused_restore_keep_lifecycle_controls() {
  let (f, boot, restore, owner) = fixture::worker("restored");
  installed(&f);
  let sessions = Sessions::new(f.manager.clone());
  sessions
    .start_restored(ID, &f.helper, boot, restore, owner)
    .await
    .unwrap();
  let rows = sessions.list_windows(Actor::Person).await.unwrap();
  assert_eq!(rows[0].state, "Running");
  assert_eq!(
    rows[0].progress.as_deref(),
    Some("Running from restored state")
  );
  sessions
    .request(ID, Actor::Person, Command::Pause {})
    .await
    .unwrap();
  let rows = sessions.list_windows(Actor::Person).await.unwrap();
  assert_eq!(rows[0].state, "Paused");
  assert_eq!(rows[0].progress.as_deref(), Some("Restored VM is paused"));
  // Even damaged recovery metadata must leave owned hardware stoppable.
  let record = config::paths(&f.manager.root, ID)
    .unwrap()
    .root
    .join("restore.hrr");
  fs::remove_file(&record).unwrap();
  fs::write(&record, b"damaged").unwrap();
  let rows = sessions.list_windows(Actor::Person).await.unwrap();
  assert_eq!(rows[0].state, "Paused");
  assert!(rows[0]
    .progress
    .as_ref()
    .unwrap()
    .contains("needs recovery"));
  sessions.stop(ID, Actor::Person).await.unwrap();
}
