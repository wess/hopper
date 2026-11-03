#[allow(dead_code)]
#[path = "../startup/fixture.rs"]
mod startup;
use engine::machines::native::restore::{Phase, Record};
use machine::snapshot::{
  manifest::{Binding, Layout},
  package::Stage,
};
use sha2::{Digest, Sha256};
pub use startup::{Fixture, ID};
use std::{
  fs::{self, File, Permissions},
  os::unix::fs::PermissionsExt,
  path::Path,
};

pub fn record() -> Record {
  Record {
    machine: model::native::machine_identity(ID).unwrap(),
    generation: [4; 16],
    snapshot: [2; 16],
    configuration: [3; 32],
    rollback: [5; 16],
    previous: None,
    phase: Phase::Prepared,
  }
}

pub fn layout() -> Layout {
  Layout {
    cpus: 1,
    banks: [(0, 65536), (65536, 65536), (1 << 30, 65536)],
    devices: [0; 7],
  }
}

pub fn private(path: &Path) -> File {
  fs::create_dir(path).unwrap();
  fs::set_permissions(path, Permissions::from_mode(0o700)).unwrap();
  File::open(path).unwrap()
}

pub fn identity(id: [u8; 16]) -> String {
  id.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn artifacts(root: &Path) {
  let r = record();
  let snapshots = private(&root.join("snapshots"));
  for snapshot in [r.snapshot, r.rollback] {
    let mut stage = Stage::new(
      &snapshots,
      Binding {
        machine: r.machine,
        snapshot,
        configuration: r.configuration,
      },
      layout(),
      || Ok(()),
    )
    .unwrap();
    // metadata fixture only; these payloads are not admitted guest checkpoints.
    let mut bytes = Vec::from(b"synthetic record");
    bytes.extend_from_slice(&Sha256::digest(&bytes));
    for component in layout().records().unwrap() {
      stage
        .write(component, || Ok(()), |out| Ok(out.write_all(&bytes)?))
        .unwrap();
    }
    drop(stage.publish(|| Ok(()), || Ok(())).unwrap());
  }
  private(&root.join("generations"));
  let path = root.join("generations").join(identity(r.generation));
  private(&path);
  let mut bytes = Vec::from(b"HGR1");
  bytes.extend_from_slice(&r.generation);
  bytes.extend_from_slice(
    &fs::read(
      root
        .join("snapshots")
        .join(identity(r.snapshot))
        .join("manifest.hsm"),
    )
    .unwrap(),
  );
  bytes.extend_from_slice(&Sha256::digest(&bytes));
  fs::write(path.join("generation.hgr"), bytes).unwrap();
  fs::set_permissions(path.join("generation.hgr"), Permissions::from_mode(0o400)).unwrap();
}

#[allow(dead_code)]
pub fn worker(
  mode: &str,
) -> (
  Fixture,
  model::native::Boot,
  model::native::Restore,
  engine::machines::native::restore::Owner,
) {
  let f = Fixture::new();
  let paths = f.initialize();
  artifacts(&paths.root);
  private(&paths.variables);
  let mut owner =
    engine::machines::native::restore::Owner::agent(f.manager.clone(), ID, 0).unwrap();
  owner.select(record(), layout(), || Ok(())).unwrap();
  owner.attempted(record(), || Ok(())).unwrap();
  let boot = model::native::Boot {
    machine_id: ID.into(),
    firmware: f.firmware.join("windows.fd").to_string_lossy().into(),
    variables: f.firmware.join("variables.fd").to_string_lossy().into(),
    store: paths.variables.to_string_lossy().into(),
    boot_media: None,
    installer: None,
    disk: None,
    disk_id: mode.into(),
    memory: 2 << 30,
    cpus: 2,
    timeout_ms: None,
  };
  let restore = model::native::Restore {
    snapshot_id: "02020202-0202-0202-0202-020202020202".into(),
    generation_id: "04040404-0404-0404-0404-040404040404".into(),
    policy_generation: 0,
  };
  (f, boot, restore, owner)
}
