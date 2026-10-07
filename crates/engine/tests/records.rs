#![cfg(unix)]

#[allow(dead_code)]
#[path = "installation/fixture.rs"]
mod fixture;

use engine::machines::{native::sessions::Sessions, Actor};
use fixture::ID;
use model::native::{Command, Installation, SetupPhase, SetupStatus};

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[test]
fn native_creation_persists_policy_without_allocating_disk_or_calling_legacy_helpers() {
  let f = fixture::standalone();
  let sessions = Sessions::new(f.manager.clone());
  let request = model::CreateMachine {
    name: "  Native Windows  ".into(),
    profile: "windows".into(),
    resources: model::EngineResources {
      cpus: 2,
      memory_gib: 4,
      disk_gib: 64,
    },
    installer: None,
    agent_access: true,
  };
  let mut invalid = request.clone();
  invalid.resources.cpus = 4;
  assert!(sessions.create_windows(invalid).is_err());
  let machine = sessions.create_windows(request.clone()).unwrap();
  assert_eq!(machine.name, "Native Windows");
  assert!(machine.agent_access && machine.installer.is_none());
  assert!(f.manager.machine(&machine.id, Actor::Agent).is_ok());
  assert!(!f.manager.root.join("native").exists());
  assert!(!f.manager.root.join("lima").exists());
  assert!(!f.manager.root.join("configs").exists());
  let mut disabled = request;
  disabled.agent_access = false;
  let copy = sessions.create_windows(disabled).unwrap();
  assert_ne!(machine.id, copy.id);
  assert!(f.manager.machine(&copy.id, Actor::Agent).is_err());
  assert_eq!(
    std::fs::read_dir(f.manager.root.join("records"))
      .unwrap()
      .count(),
    3
  );
}

#[tokio::test]
async fn listing_is_independent_of_legacy_helpers_and_preserves_old_instances() {
  let f = fixture::standalone();
  let sessions = Sessions::new(f.manager.clone());
  let rows = sessions.list_windows(Actor::Person).await.unwrap();
  assert_eq!(rows.len(), 1);
  assert_eq!(rows[0].state, "Not created");
  assert!(!rows[0].busy);
  f.manager.set_agent_access(ID, false).unwrap();
  assert!(sessions
    .list_windows(Actor::Agent)
    .await
    .unwrap()
    .is_empty());
  let legacy = f.manager.root.join("lima").join(ID);
  std::fs::create_dir_all(&legacy).unwrap();
  let disk = legacy.join("diffdisk");
  std::fs::write(&disk, "preserved previous disk").unwrap();
  let rows = sessions.list_windows(Actor::Person).await.unwrap();
  assert_eq!(rows[0].state, "Migration required");
  assert_eq!(
    std::fs::read_to_string(disk).unwrap(),
    "preserved previous disk"
  );
}

#[tokio::test]
async fn listing_reports_native_pause_phase_and_external_runtime_ownership() {
  let f = fixture::Fixture::new().await;
  f.sessions
    .request(ID, Actor::Person, Command::Pause {})
    .await
    .unwrap();
  f.emit(SetupStatus::Active {
    phase: SetupPhase::Apply,
  });
  f.wait(Installation::Setup {
    status: SetupStatus::Active {
      phase: SetupPhase::Apply,
    },
  })
  .await;
  let rows = f.sessions.list_windows(Actor::Person).await.unwrap();
  assert_eq!(rows[0].state, "Paused");
  assert_eq!(rows[0].progress.as_deref(), Some("Installing Windows"));
  let other = Sessions::new(f.source.manager.clone());
  let rows = other.list_windows(Actor::Person).await.unwrap();
  assert_eq!(rows[0].state, "Running elsewhere");
  assert!(rows[0].busy);
  f.sessions.stop(ID, Actor::Person).await.unwrap();
  let rows = other.list_windows(Actor::Person).await.unwrap();
  assert_eq!(rows[0].state, "Recovery required");
  assert!(!rows[0].busy);
}

#[tokio::test]
async fn listing_rejects_mismatched_record_filenames() {
  let f = fixture::standalone();
  let sessions = Sessions::new(f.manager.clone());
  std::fs::rename(f.record(), f.manager.root.join("records/wrong.json")).unwrap();
  assert!(sessions.list_windows(Actor::Person).await.is_err());
}
