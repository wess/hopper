#![cfg(unix)]

#[allow(dead_code)]
#[path = "installation/fixture.rs"]
mod fixture;

use engine::machines::{
  native::{config::Stage, installation, State},
  Actor,
};
use fixture::{Fixture, ID};
use fs2::FileExt;
use model::native::{Command, Installation, SetupPhase, SetupStatus};
use std::time::Duration;

#[test]
fn installation_records_reject_identity_tampering_public_files_and_symlinks() {
  use std::os::unix::fs::{symlink, PermissionsExt};
  let source = fixture::standalone();
  let paths = engine::machines::native::config::initialize(&source.manager, ID).unwrap();
  let path = paths.root.join("installation");
  let original = serde_json::to_vec(&serde_json::json!({
    "version": 1, "vmId": ID, "installation": Installation::Preparing {},
  }))
  .unwrap();
  std::fs::write(&path, &original).unwrap();
  std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
  assert_eq!(
    installation::read(&source.manager, ID, Actor::Person).unwrap(),
    Some(Installation::Preparing {})
  );
  let mut corrupt: serde_json::Value = serde_json::from_slice(&original).unwrap();
  corrupt["vmId"] = "8197e0f0-0603-43e9-a817-eaf7ab0327ae".into();
  let bytes = serde_json::to_vec(&corrupt).unwrap();
  std::fs::write(&path, &bytes).unwrap();
  assert!(installation::read(&source.manager, ID, Actor::Person).is_err());
  assert_eq!(std::fs::read(&path).unwrap(), bytes);
  std::fs::write(&path, &original).unwrap();
  std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
  assert!(installation::read(&source.manager, ID, Actor::Person).is_err());
  std::fs::remove_file(&path).unwrap();
  let outside = source.root.path().join("outside");
  std::fs::write(&outside, &original).unwrap();
  symlink(&outside, &path).unwrap();
  assert!(installation::read(&source.manager, ID, Actor::Person).is_err());
  assert_eq!(std::fs::read(outside).unwrap(), original);
}

#[tokio::test]
async fn dropping_registry_stops_installation_despite_a_surviving_viewer_watch() {
  let f = Fixture::new().await;
  let mut viewer = f.sessions.watch(ID).await.unwrap();
  let lock = std::fs::OpenOptions::new()
    .read(true)
    .write(true)
    .open(
      f.source
        .manager
        .root
        .join("locks")
        .join(format!("{ID}.runtime")),
    )
    .unwrap();
  let pid: i32 = std::fs::read_to_string(f.paths.variables.join("pid"))
    .unwrap()
    .parse()
    .unwrap();
  drop(f.sessions);
  tokio::time::timeout(Duration::from_secs(3), async {
    while lock.try_lock_exclusive().is_err() {
      tokio::time::sleep(Duration::from_millis(5)).await;
    }
  })
  .await
  .unwrap();
  FileExt::unlock(&lock).unwrap();
  assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
  tokio::time::timeout(Duration::from_secs(2), async {
    while !matches!(*viewer.borrow_and_update(), State::Stopped(_)) {
      viewer.changed().await.unwrap();
    }
  })
  .await
  .unwrap();
}

#[tokio::test]
async fn deployment_handoff_preserves_identity_detaches_media_and_keeps_viewer_watch() {
  let f = Fixture::new().await;
  let mut viewer = f.sessions.watch(ID).await.unwrap();
  f.written();
  f.emit(SetupStatus::Active {
    phase: SetupPhase::Apply,
  });
  f.wait(Installation::Setup {
    status: SetupStatus::Active {
      phase: SetupPhase::Apply,
    },
  })
  .await;
  f.emit(SetupStatus::Deployed {});
  f.wait(Installation::SystemStarted {}).await;
  let boots = f.boots();
  assert_eq!(boots.len(), 2);
  assert!(boots[0]["bootMedia"].is_string() && boots[0]["installer"].is_string());
  assert!(boots[1]["bootMedia"].is_null() && boots[1]["installer"].is_null());
  for field in ["disk", "diskId", "store", "memory", "cpus"] {
    assert_eq!(boots[0][field], boots[1][field]);
  }
  tokio::time::timeout(Duration::from_secs(2), async {
    while *viewer.borrow_and_update() != State::Running {
      viewer.changed().await.unwrap();
    }
  })
  .await
  .unwrap();
  assert_eq!(
    f.sessions.capture(ID, Actor::Person).await.unwrap().rgba,
    [1, 2, 3, 255]
  );
  f.sessions.stop(ID, Actor::Person).await.unwrap();
  assert_eq!(
    *viewer.borrow(),
    State::Stopped(model::native::StopReason::Requested)
  );
  assert!(matches!(
    installation::boot_stage(Some(&f.installation())).unwrap(),
    Stage::System
  ));
}

#[tokio::test]
async fn paused_completion_waits_for_resume_and_failures_preserve_recovery_state() {
  let f = Fixture::new().await;
  f.sessions
    .request(ID, Actor::Person, Command::Pause {})
    .await
    .unwrap();
  f.written();
  f.emit(SetupStatus::Deployed {});
  f.wait(Installation::Deployed {}).await;
  tokio::time::sleep(Duration::from_millis(600)).await;
  assert_eq!(f.boots().len(), 1);
  assert_eq!(
    f.sessions.state(ID, Actor::Person).await.unwrap(),
    Some(State::Paused)
  );
  std::fs::write(f.paths.variables.join("systemreject"), "").unwrap();
  f.sessions
    .request(ID, Actor::Person, Command::Resume {})
    .await
    .unwrap();
  f.wait(Installation::HandoffFailed {}).await;
  assert!(f.sessions.state(ID, Actor::Person).await.unwrap().is_none());
  let file = std::fs::OpenOptions::new()
    .read(true)
    .write(true)
    .open(
      f.source
        .manager
        .root
        .join("locks")
        .join(format!("{ID}.runtime")),
    )
    .unwrap();
  file.try_lock_exclusive().unwrap();
  FileExt::unlock(&file).unwrap();
  std::fs::remove_file(f.paths.variables.join("systemreject")).unwrap();
  f.sessions
    .start_configured(ID, &f.source.assets(), Stage::System)
    .await
    .unwrap();
  f.wait(Installation::SystemStarted {}).await;
  assert_eq!(f.boots().len(), 3);
  f.sessions.stop(ID, Actor::Person).await.unwrap();
}

#[tokio::test]
async fn failed_setup_is_durable_scoped_and_not_restarted_over_partial_data() {
  let f = Fixture::new().await;
  f.written();
  let failure = SetupStatus::Failed {
    phase: SetupPhase::Apply,
  };
  f.emit(failure);
  f.wait(Installation::Setup { status: failure }).await;
  f.source.manager.set_agent_access(ID, false).unwrap();
  assert!(f.sessions.installation(ID, Actor::Agent).is_err());
  f.sessions.stop(ID, Actor::Person).await.unwrap();
  assert_eq!(
    f.installation(),
    Installation::Interrupted { status: failure }
  );
  assert!(installation::boot_stage(Some(&f.installation())).is_err());
  let before = std::fs::read(f.paths.root.join("installation")).unwrap();
  assert!(f
    .sessions
    .deploy(ID, &f.source.assets(), &f.tools, f.progress.clone())
    .await
    .is_err());
  assert_eq!(
    std::fs::read(f.paths.root.join("installation")).unwrap(),
    before
  );
  assert_eq!(f.boots().len(), 1);
}

#[test]
fn setup_progress_rejects_regression_and_completed_or_failed_reentry() {
  let active = Installation::Setup {
    status: SetupStatus::Active {
      phase: SetupPhase::Apply,
    },
  };
  assert!(installation::observed(
    &active,
    SetupStatus::Active {
      phase: SetupPhase::Partition
    }
  )
  .is_err());
  assert!(installation::observed(&active, SetupStatus::Waiting {}).is_err());
  assert!(installation::observed(&active, SetupStatus::Deployed {}).is_ok());
  let failure = Installation::Setup {
    status: SetupStatus::Failed {
      phase: SetupPhase::Apply,
    },
  };
  assert!(installation::observed(&failure, SetupStatus::Deployed {}).is_err());
  assert!(
    installation::observed(&Installation::SystemStarted {}, SetupStatus::Waiting {}).is_err()
  );
}
