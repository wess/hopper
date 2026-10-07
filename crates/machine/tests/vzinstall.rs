#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use machine::vz::queue::{channel, Check};
use std::sync::{
  atomic::{AtomicBool, AtomicUsize, Ordering},
  Arc,
};

#[test]
fn queued_installation_rechecks_original_authorization_before_dispatch() {
  let runtime = tokio::runtime::Builder::new_current_thread()
    .build()
    .unwrap();
  let (client, mut owner) = channel();
  let allowed = Arc::new(AtomicBool::new(true));
  let access = allowed.clone();
  let check: Check = Arc::new(move || {
    anyhow::ensure!(access.load(Ordering::Acquire), "revoked original access");
    Ok(())
  });
  let installation = runtime
    .block_on(client.install_checked("fixture", "/missing.ipsw".into(), check))
    .unwrap();
  allowed.store(false, Ordering::Release);
  owner.tick();
  let error = runtime.block_on(installation.wait()).unwrap_err();
  assert!(error.to_string().contains("revoked original access"));
  assert!(!owner.active());
}

#[test]
fn dropping_queued_installation_skips_dispatch_and_releases_scope() {
  let runtime = tokio::runtime::Builder::new_current_thread()
    .build()
    .unwrap();
  let (client, mut owner) = channel();
  let calls = Arc::new(AtomicUsize::new(0));
  let held = calls.clone();
  let check: Check = Arc::new(move || {
    held.fetch_add(1, Ordering::Relaxed);
    Ok(())
  });
  let installation = runtime
    .block_on(client.install_checked("fixture", "/missing.ipsw".into(), check))
    .unwrap();
  assert_eq!(installation.fraction(), 0.0);
  drop(installation);
  owner.tick();
  assert_eq!(calls.load(Ordering::Relaxed), 1);
  assert_eq!(Arc::strong_count(&calls), 1);
  assert!(!owner.active());
}

#[test]
fn unavailable_hardware_and_owner_return_bounded_installation_errors() {
  let runtime = tokio::runtime::Builder::new_current_thread()
    .build()
    .unwrap();
  let (client, mut owner) = channel();
  let installation = owner
    .install_checked("fixture", "/missing.ipsw".into(), Arc::new(|| Ok(())))
    .unwrap();
  let error = runtime.block_on(installation.wait()).unwrap_err();
  assert!(error.to_string().contains("not owned"));
  assert!(!owner.active());
  drop(owner);
  let error = runtime
    .block_on(client.install_checked("fixture", "/missing.ipsw".into(), Arc::new(|| Ok(()))))
    .err()
    .unwrap();
  assert!(error.to_string().contains("owner is unavailable"));
}

#[test]
fn explicit_cancel_rejects_dispatch_while_retaining_a_completion_handle() {
  let runtime = tokio::runtime::Builder::new_current_thread()
    .build()
    .unwrap();
  let (client, mut owner) = channel();
  let installation = runtime
    .block_on(client.install_checked("fixture", "/missing.ipsw".into(), Arc::new(|| Ok(()))))
    .unwrap();
  installation.cancel();
  owner.tick();
  let error = runtime.block_on(installation.wait()).unwrap_err();
  assert!(error.to_string().contains("caller cancelled"));
  assert!(!owner.active());
}
