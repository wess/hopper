use super::fixture::{self, Fixture, ID};
use engine::machines::native::{self, config};
use model::native::{Command, Installation};
use std::{fs, os::unix::fs::PermissionsExt, sync::Arc, time::Duration};

pub async fn active() -> Fixture {
  let (f, boot, restore, owned) = fixture::worker("restored");
  let owned = Arc::new(owned);
  let weak = Arc::downgrade(&owned);
  let worker = native::launch_restored(&f.helper, boot, restore.clone(), owned.clone())
    .await
    .unwrap();
  native::resume_restored(&worker, restore, owned.clone())
    .await
    .unwrap();
  native::request(&worker, Command::Stop {}).await.unwrap();
  native::finished(&worker).await.unwrap();
  drop(worker);
  drop(owned);
  tokio::time::timeout(Duration::from_secs(2), async {
    while weak.upgrade().is_some() {
      tokio::time::sleep(Duration::from_millis(5)).await;
    }
  })
  .await
  .unwrap();
  installation(&f, Installation::SystemStarted {});
  f
}

pub fn installation(f: &Fixture, value: Installation) {
  let path = config::paths(&f.manager.root, ID)
    .unwrap()
    .root
    .join("installation");
  fs::write(
    &path,
    serde_json::to_vec(&serde_json::json!({
      "version": 1, "vmId": ID, "installation": value,
    }))
    .unwrap(),
  )
  .unwrap();
  fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

pub fn boots(f: &Fixture) -> String {
  fs::read_to_string(
    config::paths(&f.manager.root, ID)
      .unwrap()
      .variables
      .join("boots"),
  )
  .unwrap_or_default()
}
