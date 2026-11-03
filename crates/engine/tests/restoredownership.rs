#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "restore/fixture.rs"]
mod fixture;
use engine::machines::native::{self, restore::Owner};
use fixture::{Fixture, ID};
use model::native::{Boot, Restore};
use std::{
  fs,
  path::Path,
  sync::{Arc, Weak},
  time::Duration,
};

fn setup(mode: &str) -> (Fixture, Boot, Restore, Arc<Owner>) {
  let (f, boot, restore, owner) = fixture::worker(mode);
  (f, boot, restore, Arc::new(owner))
}

async fn until(mut ready: impl FnMut() -> bool) {
  tokio::time::timeout(Duration::from_secs(4), async {
    while !ready() {
      tokio::time::sleep(Duration::from_millis(5)).await;
    }
  })
  .await
  .expect("worker gate timed out");
}
async fn gate(root: &Path, name: &str) {
  until(|| {
    let path = root.join(name);
    if name == "pid" {
      return fs::read_to_string(path)
        .ok()
        .and_then(|value| value.trim().parse::<i32>().ok())
        .is_some();
    }
    path.exists()
  })
  .await;
}
fn alive(root: &Path) -> bool {
  let pid: i32 = fs::read_to_string(root.join("pid"))
    .unwrap()
    .trim()
    .parse()
    .unwrap();
  unsafe { libc::kill(pid, 0) == 0 }
}
fn held(f: &Fixture, owner: &Weak<Owner>, root: &Path) {
  assert!(alive(root));
  assert!(owner.upgrade().is_some());
  assert!(Owner::person(f.manager.clone(), ID).is_err());
}
async fn released(f: &Fixture, owner: Weak<Owner>, root: &Path) {
  until(|| !alive(root) && owner.upgrade().is_none()).await;
  drop(Owner::person(f.manager.clone(), ID).unwrap());
}

#[tokio::test]
async fn cancelling_startup_reaps_before_releasing_restore_owner() {
  let (f, boot, restore, owner) = setup("restoredstartupgate");
  let root = std::path::PathBuf::from(&boot.store);
  let weak = Arc::downgrade(&owner);
  let helper = f.helper.clone();
  let task =
    tokio::spawn(async move { native::launch_restored(&helper, boot, restore, owner).await });
  gate(&root, "pid").await;
  held(&f, &weak, &root);
  task.abort();
  assert!(task.await.err().unwrap().is_cancelled());
  released(&f, weak, &root).await;
}

#[tokio::test]
async fn revocation_can_interrupt_reconstruction_and_reaps_worker() {
  let (f, boot, restore, owner) = setup("restoredstartupgate");
  let root = std::path::PathBuf::from(&boot.store);
  let weak = Arc::downgrade(&owner);
  let helper = f.helper.clone();
  let task =
    tokio::spawn(async move { native::launch_restored(&helper, boot, restore, owner).await });
  gate(&root, "pid").await;
  f.manager.set_agent_access(ID, false).unwrap();
  assert!(tokio::time::timeout(Duration::from_secs(4), task)
    .await
    .unwrap()
    .unwrap()
    .is_err());
  released(&f, weak, &root).await;
}

#[tokio::test]
async fn final_client_drop_retains_owner_past_stop_acknowledgement() {
  let (f, boot, restore, owner) = setup("restoredstopdelay");
  let root = std::path::PathBuf::from(&boot.store);
  let weak = Arc::downgrade(&owner);
  let client = native::launch_restored(&f.helper, boot, restore, owner)
    .await
    .unwrap();
  drop(client);
  gate(&root, "stopreceived").await;
  held(&f, &weak, &root);
  fs::write(root.join("stopcontinue"), []).unwrap();
  // the fake worker records its stop reply before waiting for permission to exit.
  gate(&root, "stopacknowledged").await;
  held(&f, &weak, &root);
  fs::write(root.join("exitcontinue"), []).unwrap();
  released(&f, weak, &root).await;
}

#[tokio::test]
async fn cancelled_resume_drains_then_stops_and_retains_until_reaped() {
  let (f, boot, restore, owner) = setup("restoredresumedelay");
  let root = std::path::PathBuf::from(&boot.store);
  let weak = Arc::downgrade(&owner);
  let client = native::launch_restored(&f.helper, boot, restore.clone(), owner.clone())
    .await
    .unwrap();
  let task = tokio::spawn(async move { native::resume_restored(&client, restore, owner).await });
  gate(&root, "resumereceived").await;
  let record = native::restore::read(
    &fs::File::open(root.parent().unwrap()).unwrap(),
    model::native::machine_identity(ID).unwrap(),
  )
  .unwrap()
  .unwrap();
  assert_eq!(record.phase, native::restore::Phase::Active);
  assert!(f.manager.set_agent_access(ID, false).is_err());

  task.abort();
  assert!(task.await.err().unwrap().is_cancelled());
  held(&f, &weak, &root);
  fs::write(root.join("resumecontinue"), []).unwrap();
  gate(&root, "stopreceived").await;
  // the resume commit is gone before stop/reap, allowing immediate revocation.
  f.manager.set_agent_access(ID, false).unwrap();
  held(&f, &weak, &root);
  fs::write(root.join("stopcontinue"), []).unwrap();
  gate(&root, "stopacknowledged").await;
  held(&f, &weak, &root);
  fs::write(root.join("exitcontinue"), []).unwrap();
  released(&f, weak, &root).await;
  let reopened = Owner::person(f.manager.clone(), ID).unwrap();
  assert_eq!(
    reopened.selected().unwrap().unwrap().phase,
    native::restore::Phase::Active
  );
  assert!(native::config::initialize(&f.manager, ID).is_err());
}

#[tokio::test]
async fn mismatched_configuration_is_reaped_before_startup_returns() {
  let (f, boot, restore, owner) = setup("restoredwrongconfig");
  let root = std::path::PathBuf::from(&boot.store);
  let weak = Arc::downgrade(&owner);
  assert!(native::launch_restored(&f.helper, boot, restore, owner)
    .await
    .is_err());
  released(&f, weak, &root).await;
}

#[tokio::test]
async fn foreign_boot_namespace_is_rejected_before_worker_creation() {
  let (f, mut boot, restore, owner) = setup("restored");
  let root = std::path::PathBuf::from(&boot.store);
  let foreign = Fixture::new();
  let paths = foreign.initialize();
  fixture::private(&paths.variables);
  boot.store = paths.variables.to_string_lossy().into();
  assert!(native::launch_restored(&f.helper, boot, restore, owner)
    .await
    .is_err());
  assert!(!root.join("pid").exists());
  assert!(!paths.variables.join("pid").exists());
  drop(Owner::person(f.manager.clone(), ID).unwrap());
}

#[tokio::test]
async fn namespace_replacement_during_reconstruction_reaps_the_original_worker() {
  let (f, boot, restore, owner) = setup("restoredstartupgate");
  let root = std::path::PathBuf::from(&boot.store);
  let original = root.parent().unwrap().to_owned();
  let saved = original.with_extension("saved");
  let weak = Arc::downgrade(&owner);
  let helper = f.helper.clone();
  let task =
    tokio::spawn(async move { native::launch_restored(&helper, boot, restore, owner).await });
  gate(&root, "pid").await;
  fs::rename(&original, &saved).unwrap();
  fixture::private(&original);
  assert!(tokio::time::timeout(Duration::from_secs(4), task)
    .await
    .unwrap()
    .unwrap()
    .is_err());
  released(&f, weak, &saved.join("variables")).await;
  assert!(!original.join("variables").exists());
  assert!(saved.join("restore.hrr").exists());
}

#[tokio::test]
async fn namespace_replacement_after_admission_blocks_activation_and_execution() {
  let (f, boot, restore, owner) = setup("restored");
  let root = std::path::PathBuf::from(&boot.store);
  let original = root.parent().unwrap().to_owned();
  let saved = original.with_extension("saved");
  let weak = Arc::downgrade(&owner);
  let client = native::launch_restored(&f.helper, boot, restore.clone(), owner.clone())
    .await
    .unwrap();
  fs::rename(&original, &saved).unwrap();
  fixture::private(&original);
  assert!(native::resume_restored(&client, restore, owner)
    .await
    .is_err());
  drop(client);
  released(&f, weak, &saved.join("variables")).await;
  let record = native::restore::read(
    &fs::File::open(&saved).unwrap(),
    model::native::machine_identity(ID).unwrap(),
  )
  .unwrap()
  .unwrap();
  assert_eq!(record.phase, native::restore::Phase::Attempted);
  assert!(!original.join("restore.hrr").exists());
}

#[tokio::test]
async fn paused_activation_rejects_changed_policy_or_handoff_binding_and_reaps() {
  for revoked in [false, true] {
    let (f, boot, mut restore, owner) = setup("restored");
    let root = std::path::PathBuf::from(&boot.store);
    let weak = Arc::downgrade(&owner);
    let client = native::launch_restored(&f.helper, boot, restore.clone(), owner.clone())
      .await
      .unwrap();
    if revoked {
      f.manager.set_agent_access(ID, false).unwrap();
      f.manager.set_agent_access(ID, true).unwrap();
    } else {
      restore.policy_generation += 1;
    }
    assert!(native::pause_restored(&client, restore, owner)
      .await
      .is_err());
    assert!(!fs::read_to_string(root.join("trace"))
      .unwrap_or_default()
      .contains("restorePause"));
    drop(client);
    released(&f, weak, &root).await;
    assert_eq!(
      Owner::person(f.manager.clone(), ID)
        .unwrap()
        .selected()
        .unwrap()
        .unwrap()
        .phase,
      native::restore::Phase::Attempted
    );
  }
}

#[tokio::test]
async fn active_generation_cannot_replay_its_checkpoint_after_worker_reaping() {
  let (f, boot, restore, owner) = setup("restored");
  let root = std::path::PathBuf::from(&boot.store);
  let weak = Arc::downgrade(&owner);
  let client = native::launch_restored(&f.helper, boot.clone(), restore.clone(), owner.clone())
    .await
    .unwrap();
  native::resume_restored(&client, restore.clone(), owner.clone())
    .await
    .unwrap();
  native::request(&client, model::native::Command::Stop {})
    .await
    .unwrap();
  native::finished(&client).await.unwrap();
  drop(client);
  drop(owner);
  released(&f, weak, &root).await;
  let boots = fs::read(root.join("boots")).unwrap();
  let reopened = Arc::new(Owner::person(f.manager.clone(), ID).unwrap());
  assert_eq!(
    reopened.selected().unwrap().unwrap().phase,
    native::restore::Phase::Active
  );
  assert!(native::launch_restored(&f.helper, boot, restore, reopened)
    .await
    .is_err());
  assert_eq!(fs::read(root.join("boots")).unwrap(), boots);
  assert!(!alive(&root));
}
