#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[allow(dead_code)]
#[path = "restore/fixture.rs"]
mod fixture;
use engine::machines::{
  native::{config, restore::Owner, sessions::Sessions, State},
  Actor,
};
use fixture::ID;
#[path = "restore/active.rs"]
mod active;
use active::{active, boots, installation};
use model::native::Installation;
use sha2::{Digest, Sha256};
use std::{fs, os::unix::fs::PermissionsExt};

#[tokio::test]
async fn configured_start_uses_current_generation_without_opening_original_disk() {
  let f = active().await;
  let paths = config::paths(&f.manager.root, ID).unwrap();
  let journal = fs::read(paths.root.join("restore.hrr")).unwrap();
  fs::rename(&paths.disk, paths.disk.with_extension("saved")).unwrap();
  f.manager.set_agent_access(ID, false).unwrap();
  let sessions = Sessions::new(f.manager.clone());
  sessions
    .start_configured(ID, &f.assets(), config::Stage::System)
    .await
    .unwrap();
  assert!(matches!(
    *sessions.watch(ID).await.unwrap().borrow(),
    State::Running
  ));
  let boot: model::native::Boot = serde_json::from_str(boots(&f).lines().last().unwrap()).unwrap();
  assert_eq!(boot.machine_id, ID);
  assert_eq!(boot.cpus, 2);
  assert_eq!(boot.memory, 2 << 30);
  let digest = format!(
    "{:x}",
    Sha256::digest(uuid::Uuid::parse_str(ID).unwrap().as_bytes())
  );
  assert_eq!(boot.disk_id, format!("hpr{}", &digest[..17]));
  assert_eq!(boot.disk.as_deref(), paths.disk.to_str());
  assert!(boot.boot_media.is_none() && boot.installer.is_none());
  assert!(paths.variables.join("generationreceived").exists());
  assert!(sessions.capture(ID, Actor::Agent).await.is_err());
  sessions.stop(ID, Actor::Person).await.unwrap();
  assert!(!paths.disk.exists());
  assert_eq!(fs::read(paths.root.join("restore.hrr")).unwrap(), journal);
  assert!(config::initialize(&f.manager, ID).is_err());
  drop(Owner::person(f.manager.clone(), ID).unwrap());
}

#[tokio::test]
async fn configured_start_rejects_pending_restore_and_deployment_without_fallback() {
  let (f, _, _, owned) = fixture::worker("generation");
  drop(owned);
  installation(&f, Installation::SystemStarted {});
  let sessions = Sessions::new(f.manager.clone());
  assert!(sessions
    .start_configured(ID, &f.assets(), config::Stage::System)
    .await
    .is_err());
  assert!(boots(&f).is_empty());
  let f = active().await;
  let before = boots(&f);
  let sessions = Sessions::new(f.manager.clone());
  assert!(sessions
    .start_configured(ID, &f.assets(), config::Stage::Deployment)
    .await
    .is_err());
  installation(&f, Installation::Preparing {});
  assert!(sessions
    .start_configured(ID, &f.assets(), config::Stage::System)
    .await
    .is_err());
  assert_eq!(boots(&f), before);
}

#[tokio::test]
async fn configured_start_preserves_corrupt_selection_and_rejects_changed_assets() {
  let f = active().await;
  let paths = config::paths(&f.manager.root, ID).unwrap();
  let receipt = paths.root.join("restore.hrr");
  let good = fs::read(&receipt).unwrap();
  let mut bad = good.clone();
  bad[4] ^= 1;
  fs::set_permissions(&receipt, fs::Permissions::from_mode(0o600)).unwrap();
  fs::write(&receipt, &bad).unwrap();
  fs::set_permissions(&receipt, fs::Permissions::from_mode(0o400)).unwrap();
  let before = boots(&f);
  let sessions = Sessions::new(f.manager.clone());
  let assets = f.assets();
  assert!(sessions
    .start_configured(ID, &assets, config::Stage::System)
    .await
    .is_err());
  assert_eq!(fs::read(&receipt).unwrap(), bad);
  assert_eq!(boots(&f), before);
  fs::set_permissions(&receipt, fs::Permissions::from_mode(0o600)).unwrap();
  fs::write(&receipt, good).unwrap();
  fs::set_permissions(&receipt, fs::Permissions::from_mode(0o400)).unwrap();
  fs::write(f.firmware.join("windows.fd"), [9; 32]).unwrap();
  assert!(sessions
    .start_configured(ID, &assets, config::Stage::System)
    .await
    .is_err());
  assert_eq!(boots(&f), before);
  assert!(sessions.state(ID, Actor::Person).await.unwrap().is_none());
  drop(Owner::person(f.manager.clone(), ID).unwrap());
}
