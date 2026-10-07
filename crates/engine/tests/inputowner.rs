#![cfg(unix)]

#[allow(dead_code)]
#[path = "sessions/fixture.rs"]
mod fixture;

use engine::machines::{native::sessions::input::Input, Actor};
use fixture::{input, Fixture, ID};
use model::native::Command;
use std::time::Duration;

async fn acquire(f: &Fixture, actor: Actor) -> Input {
  tokio::time::timeout(Duration::from_secs(3), async {
    loop {
      if let Ok(owner) = f.sessions.acquire_input(ID, actor).await {
        return owner;
      }
      tokio::time::sleep(Duration::from_millis(5)).await;
    }
  })
  .await
  .unwrap()
}

#[tokio::test]
async fn input_is_exclusive_and_raw_commands_cannot_bypass_ownership() {
  let f = Fixture::new();
  f.start("normal").await;
  let person = f.sessions.acquire_input(ID, Actor::Person).await.unwrap();
  assert!(f.sessions.acquire_input(ID, Actor::Agent).await.is_err());
  assert!(f
    .sessions
    .request(ID, Actor::Person, input())
    .await
    .is_err());
  assert!(f
    .sessions
    .request(ID, Actor::Agent, Command::Release {})
    .await
    .is_err());
  assert!(person.send(Command::Status {}).await.is_err());
  person.send(input()).await.unwrap();
  person.close().await.unwrap();
  let agent = f.sessions.acquire_input(ID, Actor::Agent).await.unwrap();
  agent.send(input()).await.unwrap();
  agent.close().await.unwrap();
  assert_eq!(f.trace(), "input\nrelease\ninput\nrelease\n");
  f.sessions.stop(ID, Actor::Person).await.unwrap();
}

#[tokio::test]
async fn dropping_a_previous_owner_cannot_release_a_new_owners_keys() {
  let f = Fixture::new();
  f.start("inputdelay").await;
  let old = f.sessions.acquire_input(ID, Actor::Person).await.unwrap();
  let pending = tokio::spawn(async move { old.send(input()).await });
  f.marker("inputreceived").await;
  pending.abort();
  let _ = pending.await;
  assert!(f.sessions.acquire_input(ID, Actor::Person).await.is_err());
  f.signal("inputcontinue");
  let new = acquire(&f, Actor::Person).await;
  new.send(input()).await.unwrap();
  let trace = f.trace();
  assert!(trace.starts_with("input\nrelease\n"));
  assert!(trace.ends_with("input\n"));
  tokio::time::sleep(Duration::from_millis(30)).await;
  assert_eq!(f.trace(), trace);
  new.close().await.unwrap();
  f.sessions.stop(ID, Actor::Person).await.unwrap();
}

#[tokio::test]
async fn revocation_releases_idle_agent_ownership_without_blocking_the_person() {
  let f = Fixture::new();
  f.start("normal").await;
  let agent = f.sessions.acquire_input(ID, Actor::Agent).await.unwrap();
  agent.send(input()).await.unwrap();
  f.manager.set_agent_access(ID, false).unwrap();
  let person = acquire(&f, Actor::Person).await;
  assert!(!agent.is_active());
  assert!(agent.send(input()).await.is_err());
  agent.close().await.unwrap();
  assert!(f.sessions.acquire_input(ID, Actor::Agent).await.is_err());
  person.send(input()).await.unwrap();
  person.close().await.unwrap();
  assert_eq!(f.trace(), "input\nrelease\ninput\nrelease\n");
  f.sessions.stop(ID, Actor::Person).await.unwrap();
}

#[tokio::test]
async fn inactivity_releases_keys_and_allows_another_connection() {
  let f = Fixture::new();
  f.start("normal").await;
  let old = f.sessions.acquire_input(ID, Actor::Person).await.unwrap();
  old.send(input()).await.unwrap();
  tokio::time::sleep(Duration::from_millis(5200)).await;
  assert!(!old.is_active());
  let new = f.sessions.acquire_input(ID, Actor::Agent).await.unwrap();
  new.send(input()).await.unwrap();
  let trace = f.trace();
  old.close().await.unwrap();
  assert_eq!(f.trace(), trace);
  new.close().await.unwrap();
  f.sessions.stop(ID, Actor::Person).await.unwrap();
}

#[tokio::test]
async fn previous_worker_cleanup_cannot_touch_input_after_restart() {
  let f = Fixture::new();
  f.start("normal").await;
  let old = f.sessions.acquire_input(ID, Actor::Person).await.unwrap();
  old.send(input()).await.unwrap();
  f.sessions.stop(ID, Actor::Person).await.unwrap();
  f.start("normal").await;
  let new = acquire(&f, Actor::Person).await;
  new.send(input()).await.unwrap();
  let trace = f.trace();
  old.close().await.unwrap();
  assert_eq!(f.trace(), trace);
  new.close().await.unwrap();
  f.sessions.stop(ID, Actor::Person).await.unwrap();
}

#[tokio::test]
async fn failed_release_blocks_new_input_until_worker_replacement() {
  let f = Fixture::new();
  f.start("reject").await;
  let owner = f.sessions.acquire_input(ID, Actor::Person).await.unwrap();
  owner.send(input()).await.unwrap();
  assert!(owner.close().await.is_err());
  assert!(f
    .sessions
    .acquire_input(ID, Actor::Person)
    .await
    .err()
    .unwrap()
    .to_string()
    .contains("release failed"));
  f.sessions.stop(ID, Actor::Person).await.unwrap();
  f.start("normal").await;
  let owner = f.sessions.acquire_input(ID, Actor::Person).await.unwrap();
  owner.close().await.unwrap();
  f.sessions.stop(ID, Actor::Person).await.unwrap();
}

#[tokio::test]
async fn an_idle_input_lease_does_not_retain_the_vm_registry() {
  let f = Fixture::new();
  f.start("normal").await;
  let owner = f.sessions.acquire_input(ID, Actor::Person).await.unwrap();
  let lease = f.lock(".runtime");
  drop(f.sessions);
  fixture::wait_lock(&lease, false).await;
  owner.close().await.unwrap();
}

#[tokio::test]
async fn paused_workers_end_ownership_and_reject_input_before_dispatch() {
  let f = Fixture::new();
  f.start("normal").await;
  let owner = f.sessions.acquire_input(ID, Actor::Person).await.unwrap();
  owner.send(input()).await.unwrap();
  f.sessions
    .request(ID, Actor::Person, Command::Pause {})
    .await
    .unwrap();
  assert!(owner.send(input()).await.is_err());
  owner.close().await.unwrap();
  assert_eq!(f.trace().lines().filter(|line| *line == "input").count(), 1);
  f.sessions
    .request(ID, Actor::Person, Command::Resume {})
    .await
    .unwrap();
  let next = f.sessions.acquire_input(ID, Actor::Person).await.unwrap();
  next.send(input()).await.unwrap();
  next.close().await.unwrap();
  assert_eq!(f.trace().lines().filter(|line| *line == "input").count(), 2);
  f.sessions.stop(ID, Actor::Person).await.unwrap();
}

#[tokio::test]
async fn an_agent_lease_cannot_survive_an_off_on_policy_toggle() {
  let f = Fixture::new();
  f.start("normal").await;
  let old = f.sessions.acquire_input(ID, Actor::Agent).await.unwrap();
  f.manager.set_agent_access(ID, false).unwrap();
  f.manager.set_agent_access(ID, true).unwrap();
  assert!(old.send(input()).await.is_err());
  old.close().await.unwrap();
  let fresh = f.sessions.acquire_input(ID, Actor::Agent).await.unwrap();
  fresh.send(input()).await.unwrap();
  fresh.close().await.unwrap();
  assert_eq!(f.trace().lines().filter(|line| *line == "input").count(), 1);
  f.sessions.stop(ID, Actor::Person).await.unwrap();
}
