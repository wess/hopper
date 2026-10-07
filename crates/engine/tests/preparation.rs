#![cfg(unix)]

#[path = "support/credentials.rs"]
mod backend;
#[allow(dead_code)]
#[path = "setup/fixture.rs"]
mod media;
#[allow(dead_code)]
#[path = "startup/fixture.rs"]
mod startup;

use engine::machines::native::{
  config,
  deployment::{Phase, Tools},
  sessions::Sessions,
};
use engine::machines::Actor;
use std::{
  io::Write,
  os::unix::fs::PermissionsExt,
  sync::{atomic::Ordering, Arc},
  time::Duration,
};
use tokio::sync::watch;

fn tooling(media: &media::Fixture) -> Tools {
  let mount = media.root.path().join("mount");
  std::fs::write(&mount, include_str!("inspect/tool.py")).unwrap();
  std::fs::set_permissions(&mount, std::fs::Permissions::from_mode(0o700)).unwrap();
  let input = media.input();
  Tools {
    media: media.tools.clone(),
    mount,
    drivers: input.drivers.into(),
    license: input.license.into(),
  }
}

async fn wait(path: &std::path::Path) {
  tokio::time::timeout(Duration::from_secs(3), async {
    while !path.exists() {
      tokio::time::sleep(Duration::from_millis(5)).await;
    }
  })
  .await
  .unwrap();
}

#[tokio::test]
async fn preparation_retries_preserve_media_credentials_and_existing_disk_data() {
  let credentials = Arc::new(backend::State::default());
  keyring::set_default_credential_builder(Box::new(backend::Builder(credentials.clone())));
  let f = startup::Fixture::new();
  let media = media::Fixture::new("fail");
  let tools = tooling(&media);
  let paths = config::initialize(&f.manager, startup::ID).unwrap();
  std::fs::create_dir(&paths.variables).unwrap();
  let sessions = Sessions::new(f.manager.clone());
  let (progress, status) = watch::channel(Phase::Inspecting);
  assert!(sessions
    .deploy(startup::ID, &f.assets(), &tools, progress.clone())
    .await
    .is_err());
  assert!(!paths.disk.exists() && !paths.setup.exists());
  assert_eq!(credentials.writes.load(Ordering::SeqCst), 1);
  std::fs::write(media.root.path().join("mode"), "normal").unwrap();
  sessions
    .deploy(startup::ID, &f.assets(), &tools, progress.clone())
    .await
    .unwrap();
  assert!(matches!(*status.borrow(), Phase::Starting));
  assert_eq!(credentials.writes.load(Ordering::SeqCst), 1);
  sessions.stop(startup::ID, Actor::Person).await.unwrap();
  let original = std::fs::read(&paths.setup).unwrap();
  // a completed bundle can be reused even if the update helper now fails.
  std::fs::write(media.root.path().join("mode"), "fail").unwrap();
  sessions
    .deploy(startup::ID, &f.assets(), &tools, progress.clone())
    .await
    .unwrap();
  sessions.stop(startup::ID, Actor::Person).await.unwrap();
  assert_eq!(std::fs::read(&paths.setup).unwrap(), original);
  assert_eq!(credentials.writes.load(Ordering::SeqCst), 1);
  let installer = f.root.path().join("installer.iso");
  std::fs::write(&installer, [7; 32]).unwrap();
  assert!(sessions
    .deploy(startup::ID, &f.assets(), &tools, progress.clone())
    .await
    .is_err());
  std::fs::write(&installer, [1; 32]).unwrap();
  std::fs::write(media.root.path().join("mode"), "large").unwrap();
  assert!(sessions
    .deploy(startup::ID, &f.assets(), &tools, progress.clone())
    .await
    .is_err());
  assert_eq!(std::fs::read(&paths.setup).unwrap(), original);
  std::fs::write(media.root.path().join("mode"), "fail").unwrap();
  std::fs::OpenOptions::new()
    .write(true)
    .open(&paths.disk)
    .unwrap()
    .write_all(b"preserve partial deployment")
    .unwrap();
  assert!(sessions
    .deploy(startup::ID, &f.assets(), &tools, progress.clone())
    .await
    .is_err());
  let mut disk = std::fs::File::open(&paths.disk).unwrap();
  use std::io::Read;
  let mut bytes = [0; 27];
  disk.read_exact(&mut bytes).unwrap();
  assert_eq!(&bytes, b"preserve partial deployment");
  assert_eq!(std::fs::read(&paths.setup).unwrap(), original);
  assert_eq!(credentials.writes.load(Ordering::SeqCst), 1);

  let cancelled = startup::Fixture::new();
  let pending_media = media::Fixture::new("gate");
  let pending_tools = tooling(&pending_media);
  let sessions = Arc::new(Sessions::new(cancelled.manager.clone()));
  let work = sessions.clone();
  let assets = cancelled.assets();
  let start = tokio::spawn(async move {
    work
      .deploy(startup::ID, &assets, &pending_tools, progress)
      .await
  });
  wait(&pending_media.root.path().join("waiting")).await;
  start.abort();
  assert!(matches!(start.await, Err(error) if error.is_cancelled()));
  assert!(lease(&cancelled.manager.root, ".runtime").is_err());
  assert!(lease(&cancelled.manager.root, "").is_err());
  std::fs::write(pending_media.root.path().join("continue"), b"").unwrap();
  tokio::time::timeout(Duration::from_secs(3), async {
    loop {
      if let Ok(lease) = lease(&cancelled.manager.root, ".runtime") {
        drop(lease);
        break;
      }
      tokio::time::sleep(Duration::from_millis(5)).await;
    }
  })
  .await
  .unwrap();
  let paths = config::paths(&cancelled.manager.root, startup::ID).unwrap();
  assert!(paths.setup.exists() && paths.disk.exists());
  assert!(!paths.variables.join("pid").exists());
  assert!(sessions
    .state(startup::ID, Actor::Person)
    .await
    .unwrap()
    .is_none());
}

fn lease(root: &std::path::Path, suffix: &str) -> anyhow::Result<std::fs::File> {
  let path = root.join("locks").join(format!("{}{suffix}", startup::ID));
  let file = std::fs::OpenOptions::new()
    .read(true)
    .write(true)
    .open(path)?;
  fs2::FileExt::try_lock_exclusive(&file)?;
  Ok(file)
}
