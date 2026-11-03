#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use machine::vz::queue::{channel, Check};
use model::guest::wire::Action;
use std::sync::{
  atomic::{AtomicBool, Ordering},
  Arc,
};

fn action() -> Action {
  Action::Poll {
    job: "4ae3943e-9d15-44f2-b0f5-5729bc464c0f".into(),
  }
}

#[tokio::test]
async fn queued_guest_requests_recheck_original_authorization_before_dispatch() {
  let (client, mut owner) = channel();
  let allowed = Arc::new(AtomicBool::new(true));
  let original = allowed.clone();
  let check: Check = Arc::new(move || {
    anyhow::ensure!(original.load(Ordering::Acquire), "revoked");
    Ok(())
  });
  let task = tokio::spawn(async move {
    client
      .guest_request("linux", 1, 7, action(), (check.clone(), check))
      .await
  });
  owner.wake().notified().await;
  allowed.store(false, Ordering::Release);
  owner.tick();
  assert!(task
    .await
    .unwrap()
    .unwrap_err()
    .to_string()
    .contains("revoked"));
  assert!(!owner.active());
}

#[tokio::test]
async fn cancelled_and_unowned_requests_do_not_leave_active_connections() {
  let (client, mut owner) = channel();
  let check: Check = Arc::new(|| Ok(()));
  let copy = client.clone();
  let checks = (check.clone(), check.clone());
  let task = tokio::spawn(async move { copy.guest_request("linux", 1, 7, action(), checks).await });
  owner.wake().notified().await;
  task.abort();
  assert!(task.await.unwrap_err().is_cancelled());
  owner.tick();
  assert!(!owner.active());
  let task = tokio::spawn(async move {
    client
      .guest_request("linux", 1, 7, action(), (check.clone(), check))
      .await
  });
  owner.wake().notified().await;
  owner.tick();
  assert!(task
    .await
    .unwrap()
    .unwrap_err()
    .to_string()
    .contains("not owned"));
  assert!(!owner.active());
}

#[tokio::test]
async fn invalid_guest_requests_reject_without_waking_the_owner() {
  let (client, owner) = channel();
  let check: Check = Arc::new(|| Ok(()));
  for (id, generation, action) in [
    ("../linux", 1, action()),
    ("linux", 0, action()),
    (
      "linux",
      1,
      Action::Cancel {
        job: "../another".into(),
      },
    ),
  ] {
    assert!(client
      .guest_request(id, generation, 7, action, (check.clone(), check.clone()))
      .await
      .is_err());
  }
  assert!(tokio::time::timeout(
    std::time::Duration::from_millis(10),
    owner.wake().notified()
  )
  .await
  .is_err());
}
