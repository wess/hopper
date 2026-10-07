use engine::machines::{
  vz::{mac::platform, records},
  Machines,
};
use machine::vz::restore::Image;
use std::os::unix::fs::PermissionsExt;

pub fn fixture() -> (
  tempfile::TempDir,
  Machines,
  model::Machine,
  Image,
  std::path::PathBuf,
) {
  let root = tempfile::tempdir().unwrap();
  let manager = Machines {
    root: root.path().join("machines"),
  };
  let machine = records::create(
    &manager,
    model::CreateMachine {
      name: "Deployment fixture".into(),
      profile: "macos".into(),
      resources: model::EngineResources {
        cpus: 2,
        memory_gib: 4,
        disk_gib: 64,
      },
      agent_access: true,
      installer: None,
    },
  )
  .unwrap();
  let image = Image {
    url: String::new(),
    build: "fixture".into(),
    version: [26, 0, 0],
    hardware: vec![1],
    minimum_cpus: 2,
    minimum_memory: 4 << 30,
  };
  let target = manager.root.join("vz").join(&machine.id);
  std::fs::create_dir_all(target.join("auxiliary")).unwrap();
  for path in [&manager.root.join("vz"), &target, &target.join("auxiliary")] {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
  }
  let saved = platform::Platform {
    id: machine.id.clone(),
    version: 1,
    disk_gib: 64,
    identity: vec![1],
    hardware: image.hardware.clone(),
    build: image.build.clone(),
    os_version: image.version,
    minimum_cpus: 2,
    minimum_memory: 4 << 30,
  };
  for (name, bytes) in [
    ("platform", serde_json::to_vec(&saved).unwrap()),
    ("auxiliary/hardware", vec![1]),
    ("auxiliary/state", b"owned state".to_vec()),
  ] {
    std::fs::write(target.join(name), bytes).unwrap();
    std::fs::set_permissions(target.join(name), std::fs::Permissions::from_mode(0o600)).unwrap();
  }
  let disk = std::fs::File::create(target.join("disk")).unwrap();
  disk.set_len(64 << 30).unwrap();
  std::fs::set_permissions(target.join("disk"), std::fs::Permissions::from_mode(0o600)).unwrap();
  (root, manager, machine, image, target)
}
