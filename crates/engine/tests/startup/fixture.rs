use engine::machines::{
  native::{assets, config},
  Machines,
};
use sha2::{Digest, Sha256};
use std::{
  os::unix::fs::{OpenOptionsExt, PermissionsExt},
  path::PathBuf,
};
use tempfile::TempDir;

pub const ID: &str = "8197e0f0-0603-43e9-a817-eaf7ab0327af";

pub struct Fixture {
  pub root: TempDir,
  pub manager: Machines,
  pub helper: PathBuf,
  pub firmware: PathBuf,
}

impl Fixture {
  pub fn new() -> Self {
    let root = tempfile::tempdir().unwrap();
    let firmware = root.path().join("firmware");
    std::fs::create_dir(&firmware).unwrap();
    std::fs::write(firmware.join("windows.fd"), [1; 32]).unwrap();
    std::fs::write(firmware.join("variables.fd"), [2; 16]).unwrap();
    let manifest = serde_json::json!({
      "size": 32, "sha256": format!("{:x}", Sha256::digest([1; 32])),
      "variables": { "size": 16, "sha256": format!("{:x}", Sha256::digest([2; 16])) }
    });
    std::fs::write(
      firmware.join("manifest.json"),
      serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    let helper = root.path().join("hoppervm");
    std::fs::write(&helper, include_str!("../native/worker.py")).unwrap();
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700)).unwrap();
    let manager = Machines {
      root: root.path().join("machines"),
    };
    let installer = root.path().join("installer.iso");
    std::fs::write(&installer, [1; 32]).unwrap();
    let machine = model::Machine {
      id: ID.into(),
      name: "Synthetic startup".into(),
      guest: model::GuestOs::Windows,
      profile: "windows".into(),
      resources: model::EngineResources {
        cpus: 2,
        memory_gib: 2,
        disk_gib: 64,
      },
      agent_access: true,
      agent_generation: 0,
      runtime: None,
      installer: Some(installer.to_string_lossy().into()),
    };
    store::json::write(
      &manager.root.join("records").join(format!("{ID}.json")),
      &machine,
    )
    .unwrap();
    Self {
      root,
      manager,
      helper,
      firmware,
    }
  }

  pub fn assets(&self) -> assets::Assets {
    assets::verify(&self.helper, &self.firmware).unwrap()
  }
  pub fn initialize(&self) -> config::Paths {
    let paths = config::initialize(&self.manager, ID).unwrap();
    let file = std::fs::OpenOptions::new()
      .write(true)
      .create_new(true)
      .mode(0o600)
      .open(&paths.disk)
      .unwrap();
    file.set_len(64 * 1024 * 1024 * 1024).unwrap();
    std::fs::write(&paths.setup, [1; 32]).unwrap();
    let metadata = paths.setup.with_extension("json");
    std::fs::write(
      &metadata,
      serde_json::to_vec(&serde_json::json!({
        "vmId": ID, "size": 32, "sha256": format!("{:x}", Sha256::digest([1; 32])),
        "containsGuestCredentials": true, "detachBeforeFirstBoot": true,
      }))
      .unwrap(),
    )
    .unwrap();
    std::fs::set_permissions(&metadata, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::set_permissions(&paths.setup, std::fs::Permissions::from_mode(0o600)).unwrap();
    paths
  }
  pub fn record(&self) -> PathBuf {
    self.manager.root.join("records").join(format!("{ID}.json"))
  }
}
