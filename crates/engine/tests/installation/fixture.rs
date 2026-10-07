#[allow(dead_code)]
#[path = "../setup/fixture.rs"]
mod media;
#[allow(dead_code)]
#[path = "../startup/fixture.rs"]
mod startup;

use engine::machines::{
  native::{
    config,
    deployment::{Phase, Tools},
    sessions::Sessions,
  },
  Actor,
};
use model::native::{Installation, SetupStatus};
use std::{
  io::Write,
  os::unix::fs::PermissionsExt,
  sync::{Arc, Once},
  time::Duration,
};
use tokio::sync::watch;

pub const ID: &str = startup::ID;
static KEYRING: Once = Once::new();

pub fn standalone() -> startup::Fixture {
  startup::Fixture::new()
}

pub struct Fixture {
  pub source: startup::Fixture,
  pub media: media::Fixture,
  pub sessions: Arc<Sessions>,
  pub paths: config::Paths,
  pub tools: Tools,
  pub progress: watch::Sender<Phase>,
}

impl Fixture {
  pub async fn new() -> Self {
    KEYRING.call_once(|| {
      keyring::set_default_credential_builder(keyring::mock::default_credential_builder())
    });
    let source = startup::Fixture::new();
    let media = media::Fixture::new("normal");
    let mount = media.root.path().join("mount");
    std::fs::write(&mount, include_str!("../inspect/tool.py")).unwrap();
    std::fs::set_permissions(&mount, std::fs::Permissions::from_mode(0o700)).unwrap();
    let input = media.input();
    let tools = Tools {
      media: media.tools.clone(),
      mount,
      drivers: input.drivers.into(),
      license: input.license.into(),
    };
    let paths = config::initialize(&source.manager, ID).unwrap();
    std::fs::create_dir(&paths.variables).unwrap();
    let sessions = Arc::new(Sessions::new(source.manager.clone()));
    let (progress, _) = watch::channel(Phase::Inspecting);
    sessions
      .deploy(ID, &source.assets(), &tools, progress.clone())
      .await
      .unwrap();
    Self {
      source,
      media,
      sessions,
      paths,
      tools,
      progress,
    }
  }

  pub fn emit(&self, status: SetupStatus) {
    store::json::write(&self.paths.variables.join("setupstatus"), &status).unwrap();
  }

  pub fn written(&self) {
    std::fs::OpenOptions::new()
      .write(true)
      .open(&self.paths.disk)
      .unwrap()
      .write_all(b"synthetic deployed system")
      .unwrap();
  }

  pub fn installation(&self) -> Installation {
    self
      .sessions
      .installation(ID, Actor::Person)
      .unwrap()
      .unwrap()
  }

  pub async fn wait(&self, expected: Installation) {
    tokio::time::timeout(Duration::from_secs(5), async {
      while self.installation() != expected {
        tokio::time::sleep(Duration::from_millis(5)).await;
      }
    })
    .await
    .unwrap();
  }

  pub fn boots(&self) -> Vec<serde_json::Value> {
    std::fs::read_to_string(self.paths.variables.join("boots"))
      .unwrap()
      .lines()
      .map(|line| serde_json::from_str(line).unwrap())
      .collect()
  }
}
