#![cfg(unix)]

#[allow(dead_code)]
#[path = "installation/fixture.rs"]
mod fixture;

use engine::machines::{
  native::{config::Stage, State},
  Actor,
};
use fixture::{Fixture, ID};
use model::native::{Installation, SetupStatus};
use std::time::Duration;

#[tokio::test]
async fn system_guest_resets_restart_without_media_but_shutdown_stays_stopped() {
  let f = Fixture::new().await;
  f.written();
  f.emit(SetupStatus::Deployed {});
  f.wait(Installation::SystemStarted {}).await;
  let viewer = f.sessions.watch(ID).await.unwrap();
  let first = f.boots()[1].clone();
  for count in 3..=5 {
    std::fs::write(f.paths.variables.join("powerevent"), "reset").unwrap();
    assert!(f.sessions.capture(ID, Actor::Person).await.is_err());
    tokio::time::timeout(Duration::from_secs(5), async {
      loop {
        if f.boots().len() == count
          && f.installation() == (Installation::SystemStarted {})
          && f.sessions.state(ID, Actor::Person).await.unwrap() == Some(State::Running)
        {
          break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
      }
    })
    .await
    .unwrap();
    assert_eq!(f.boots()[count - 1], first);
    assert_eq!(*viewer.borrow(), State::Running);
  }
  std::fs::write(f.paths.variables.join("powerevent"), "shutdown").unwrap();
  assert!(f.sessions.capture(ID, Actor::Person).await.is_err());
  tokio::time::sleep(Duration::from_millis(600)).await;
  assert_eq!(f.boots().len(), 5);
  assert_eq!(
    f.sessions.state(ID, Actor::Person).await.unwrap(),
    Some(State::Stopped(model::native::StopReason::Shutdown))
  );
  f.sessions.stop(ID, Actor::Person).await.unwrap();
}

#[tokio::test]
async fn system_reset_failure_retains_retry_intent_and_manual_restart_is_supervised() {
  let f = Fixture::new().await;
  f.written();
  f.emit(SetupStatus::Deployed {});
  f.wait(Installation::SystemStarted {}).await;
  std::fs::write(f.paths.variables.join("systemreject"), "").unwrap();
  std::fs::write(f.paths.variables.join("powerevent"), "reset").unwrap();
  assert!(f.sessions.capture(ID, Actor::Person).await.is_err());
  f.wait(Installation::HandoffFailed {}).await;
  assert!(f.sessions.state(ID, Actor::Person).await.unwrap().is_none());
  std::fs::remove_file(f.paths.variables.join("systemreject")).unwrap();
  f.sessions
    .start_configured(ID, &f.source.assets(), Stage::System)
    .await
    .unwrap();
  std::fs::write(f.paths.variables.join("powerevent"), "reset").unwrap();
  assert!(f.sessions.capture(ID, Actor::Person).await.is_err());
  tokio::time::timeout(Duration::from_secs(5), async {
    while f.boots().len() != 5 || f.installation() != (Installation::SystemStarted {}) {
      tokio::time::sleep(Duration::from_millis(5)).await;
    }
  })
  .await
  .unwrap();
  f.sessions.stop(ID, Actor::Person).await.unwrap();
  tokio::time::sleep(Duration::from_millis(600)).await;
  assert_eq!(f.boots().len(), 5);
}
