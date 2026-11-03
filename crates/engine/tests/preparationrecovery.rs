#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[allow(dead_code)]
#[path = "restore/fixture.rs"]
mod fixture;
use engine::machines::native::{
  config,
  restore::{self, Owner, Phase, Recovery},
};
use fixture::ID;
use std::{
  fs,
  os::unix::fs::PermissionsExt,
  sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
  },
};

#[tokio::test]
async fn cancelled_revoked_or_unrelated_selections_never_authorize_preparation_recovery() {
  for fault in 0..4 {
    let (f, boot, _, owner) = fixture::worker("restored");
    let paths = config::paths(&f.manager.root, ID).unwrap();
    let selection = paths.root.join("restore.hrr");
    if fault == 3 {
      let mut selected = owner.selected().unwrap().unwrap();
      selected.phase = Phase::Active;
      fs::set_permissions(&selection, fs::Permissions::from_mode(0o600)).unwrap();
      fs::write(&selection, selected.encode().unwrap()).unwrap();
      fs::set_permissions(&selection, fs::Permissions::from_mode(0o400)).unwrap();
    }
    let original = fs::read(&selection).unwrap();
    let snapshots = fs::read_dir(paths.root.join("snapshots")).unwrap().count();
    if fault == 1 {
      f.manager.set_agent_access(ID, false).unwrap();
      f.manager.set_agent_access(ID, true).unwrap();
    }
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let result = restore::recover_preparation(
      owner,
      anyhow::anyhow!("injected preparation failure"),
      Recovery {
        boot,
        target: "09090909-0909-0909-0909-090909090909".into(),
        prior: "05050505-0505-0505-0505-050505050505".into(),
        predecessor: None,
        configuration: [3; 32],
        check: Arc::new(move || {
          observed.fetch_add(1, Ordering::SeqCst);
          anyhow::ensure!(fault != 0, "caller cancelled");
          Ok(())
        }),
      },
    )
    .await;
    let error = result.err().unwrap();
    assert!(format!("{error:#}").contains("injected preparation failure"));
    assert_eq!(calls.load(Ordering::SeqCst), if fault == 1 { 0 } else { 1 });
    assert_eq!(fs::read(&selection).unwrap(), original);
    assert_eq!(
      fs::read_dir(paths.root.join("snapshots")).unwrap().count(),
      snapshots
    );
    drop(Owner::person(f.manager.clone(), ID).unwrap());
  }
}
