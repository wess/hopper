#[allow(dead_code)]
#[path = "../sessions/fixture.rs"]
mod source;
use engine::machines::native::config;
use machine::snapshot::{
  manifest::{Binding, Layout},
  package::Stage,
};
use sha2::{Digest, Sha256};
pub use source::ID;
use std::{
  fs::{File, Permissions},
  os::unix::fs::PermissionsExt,
  path::PathBuf,
};

pub struct Fixture {
  pub source: source::Fixture,
  pub root: PathBuf,
  pub variables: PathBuf,
}

impl Fixture {
  pub async fn new(mode: &str) -> Self {
    let source = source::Fixture::new();
    let paths = config::initialize(&source.manager, ID).unwrap();
    std::fs::create_dir(&paths.variables).unwrap();
    std::fs::set_permissions(&paths.variables, Permissions::from_mode(0o700)).unwrap();
    let mut boot = source.boot(mode);
    boot.store = paths.variables.to_string_lossy().into();
    source
      .sessions
      .start(ID, &source.helper, boot)
      .await
      .unwrap();
    Self {
      source,
      root: paths.root,
      variables: paths.variables,
    }
  }

  pub fn marker(&self, name: &str) {
    std::fs::write(self.variables.join(name), b"").unwrap();
  }

  pub async fn wait(&self, name: &str) {
    source::wait_file(&self.variables.join(name)).await;
  }

  pub fn locked(&self) -> bool {
    use fs2::FileExt;
    let file = std::fs::OpenOptions::new()
      .read(true)
      .write(true)
      .open(self.source.manager.root.join("locks").join(ID))
      .unwrap();
    file.try_lock_exclusive().is_err()
  }

  pub async fn released(&self) {
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
      while self.locked() {
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
      }
    })
    .await
    .unwrap();
  }

  pub fn trace(&self) -> String {
    std::fs::read_to_string(self.variables.join("trace")).unwrap_or_default()
  }

  pub fn ready(&self, snapshot: &str) {
    let path = self.root.join("snapshots");
    std::fs::create_dir_all(&path).unwrap();
    std::fs::set_permissions(&path, Permissions::from_mode(0o700)).unwrap();
    let parent = File::open(path).unwrap();
    let binding = Binding {
      machine: model::native::machine_identity(ID).unwrap(),
      snapshot: model::native::machine_identity(snapshot).unwrap(),
      configuration: [7; 32],
    };
    let layout = Layout {
      cpus: 2,
      banks: [
        (0, 0x4000000),
        (0x4000000, 0x4000000),
        (machine::platform::RAM, 0x10000000),
      ],
      devices: [0, 2, 3, 3, 0, 0, 5],
    };
    let mut stage = Stage::new(&parent, binding, layout, || Ok(())).unwrap();
    let mut bytes = b"test component".to_vec();
    bytes.extend_from_slice(&Sha256::digest(&bytes));
    for record in layout.records().unwrap() {
      stage
        .write(record, || Ok(()), |out| Ok(out.write_all(&bytes)?))
        .unwrap();
    }
    let sealed = stage.seal(|| Ok(())).unwrap();
    std::fs::write(
      self.variables.join("snapshotreceipt"),
      serde_json::to_vec(&sealed.receipt()).unwrap(),
    )
    .unwrap();
  }
}
