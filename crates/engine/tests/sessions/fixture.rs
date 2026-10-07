use engine::machines::{native::sessions::Sessions, Machines};
use fs2::FileExt;
use model::native::{Boot, Command, InputDevice, InputEvent};
use std::{
  os::unix::fs::PermissionsExt,
  path::{Path, PathBuf},
  sync::Arc,
  time::Duration,
};
use tempfile::TempDir;

pub const ID: &str = "8197e0f0-0603-43e9-a817-eaf7ab0327af";

pub struct Fixture {
  pub root: TempDir,
  pub manager: Machines,
  pub sessions: Arc<Sessions>,
  pub helper: PathBuf,
}

impl Fixture {
  pub fn new() -> Self {
    // macOS temporary roots can exceed the Unix socket path limit.
    let root = tempfile::tempdir_in("/tmp").unwrap();
    let manager = Machines {
      root: root.path().join("machines"),
    };
    std::fs::create_dir_all(root.path().join("probe")).unwrap();
    let machine = model::Machine {
      id: ID.into(),
      name: "Synthetic native VM".into(),
      guest: model::GuestOs::Windows,
      profile: "windows".into(),
      resources: model::EngineResources {
        cpus: 2,
        memory_gib: 2,
        disk_gib: 64,
      },
      agent_access: true,
      agent_generation: 0,
      runtime: Some(model::MachineRuntime::Hypervisor),
      installer: None,
    };
    store::json::write(
      &manager.root.join("records").join(format!("{ID}.json")),
      &machine,
    )
    .unwrap();
    let helper = root.path().join("worker");
    std::fs::write(&helper, include_str!("../native/worker.py")).unwrap();
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700)).unwrap();
    let sessions = Arc::new(Sessions::new(manager.clone()));
    Self {
      root,
      manager,
      sessions,
      helper,
    }
  }

  pub fn boot(&self, mode: &str) -> Boot {
    Boot {
      firmware: String::new(),
      variables: String::new(),
      store: self.probe().to_string_lossy().into(),
      boot_media: None,
      installer: None,
      disk: None,
      disk_id: mode.into(),
      memory: 0x10000000,
      cpus: 2,
      timeout_ms: None,
    }
  }

  pub fn probe(&self) -> PathBuf {
    self.root.path().join("probe")
  }
  pub fn trace(&self) -> String {
    std::fs::read_to_string(self.probe().join("trace")).unwrap_or_default()
  }
  pub async fn start(&self, mode: &str) {
    self
      .sessions
      .start(ID, &self.helper, self.boot(mode))
      .await
      .unwrap();
  }
  pub fn signal(&self, name: &str) {
    std::fs::write(self.probe().join(name), []).unwrap();
  }
  pub async fn marker(&self, name: &str) {
    wait_file(&self.probe().join(name)).await;
  }
  pub fn lock(&self, suffix: &str) -> std::fs::File {
    std::fs::OpenOptions::new()
      .read(true)
      .write(true)
      .open(
        self
          .manager
          .root
          .join("locks")
          .join(format!("{ID}{suffix}")),
      )
      .unwrap()
  }
}

pub async fn wait_file(path: &Path) {
  tokio::time::timeout(Duration::from_secs(3), async {
    while !path.exists() {
      tokio::time::sleep(Duration::from_millis(5)).await;
    }
  })
  .await
  .unwrap();
}

pub async fn wait_lock(file: &std::fs::File, busy: bool) {
  tokio::time::timeout(Duration::from_secs(3), async {
    loop {
      let locked = match file.try_lock_exclusive() {
        Ok(()) => {
          FileExt::unlock(file).unwrap();
          false
        }
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => true,
        Err(error) => panic!("{error}"),
      };
      if locked == busy {
        break;
      }
      tokio::time::sleep(Duration::from_millis(5)).await;
    }
  })
  .await
  .unwrap();
}

pub fn input() -> Command {
  Command::Input {
    device: InputDevice::Keyboard,
    events: vec![InputEvent {
      kind: 1,
      code: 30,
      value: 1,
    }],
  }
}
