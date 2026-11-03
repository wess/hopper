#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
use machine::{
  runtime::hardware,
  snapshot::{
    manifest::{Binding, Layout, Record},
    package::{Package, Stage},
  },
};
use model::native::Boot;
use sha2::{Digest, Sha256};
use std::{fs, os::unix::fs::PermissionsExt};

fn disk_record(sectors: u64, readonly: bool) -> Vec<u8> {
  let mut bytes = vec![0; 142];
  bytes[..4].copy_from_slice(b"HDB1");
  bytes[4..12].copy_from_slice(&sectors.to_le_bytes());
  bytes[12] = readonly as u8;
  bytes[14..34].copy_from_slice(b"hopper00000000000001");
  bytes[34..38].copy_from_slice(b"HDS1");
  bytes[38..46].copy_from_slice(&sectors.wrapping_mul(512).to_le_bytes());
  let checksum = Sha256::digest(&bytes[34..78]);
  bytes[78..110].copy_from_slice(&checksum);
  let checksum = Sha256::digest(&bytes[..110]);
  bytes[110..].copy_from_slice(&checksum);
  bytes
}

fn fixture() -> (tempfile::TempDir, Boot, Package, [u8; 32]) {
  let root = tempfile::tempdir().unwrap();
  fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
  fs::write(root.path().join("firmware"), 0x14000000u32.to_le_bytes()).unwrap();
  fs::write(root.path().join("template"), [0xff]).unwrap();
  let disk = root.path().join("original");
  fs::write(&disk, [0; 4096]).unwrap();
  let boot = Boot {
    machine_id: "8197e0f0-0603-43e9-a817-eaf7ab0327af".into(),
    firmware: root.path().join("firmware").to_string_lossy().into(),
    variables: root.path().join("template").to_string_lossy().into(),
    store: root.path().join("absentvariables").to_string_lossy().into(),
    disk: Some(disk.to_string_lossy().into()),
    disk_id: "hopper00000000000001".into(),
    memory: 0x10000000,
    cpus: 2,
    timeout_ms: None,
    boot_media: None,
    installer: None,
  };
  let (_, devices, configuration) = hardware::original(boot.clone(), || Ok(())).unwrap();
  drop(devices);
  let layout = Layout {
    cpus: 2,
    banks: [
      (0, 0x4000000),
      (0x4000000, 0x4000000),
      (machine::platform::RAM, boot.memory),
    ],
    devices: [0, 2, 3, 3, 1, 0, 5],
  };
  let parent = fs::File::open(root.path()).unwrap();
  let binding = Binding {
    machine: model::native::machine_identity(&boot.machine_id).unwrap(),
    snapshot: [2; 16],
    configuration,
  };
  let mut stage = Stage::new(&parent, binding, layout, || Ok(())).unwrap();
  // Hardware-shape fixture only; other components deliberately cannot admit a guest checkpoint.
  for record in layout.records().unwrap() {
    let bytes = if record == Record::Backend(4) {
      disk_record(8, false)
    } else {
      let mut bytes = vec![0; 4];
      bytes.extend_from_slice(&Sha256::digest(&bytes));
      bytes
    };
    stage
      .write(record, || Ok(()), |out| Ok(out.write_all(&bytes)?))
      .unwrap();
  }
  drop(stage.publish(|| Ok(()), || Ok(())).unwrap());
  fs::remove_file(disk).unwrap();
  let package = Package::inspect(&parent, binding.machine, binding.snapshot, || Ok(())).unwrap();
  (root, boot, package, configuration)
}

#[test]
fn shape_rebuild_never_opens_original_paths_and_matches_independent_configuration() {
  let (_root, boot, package, configuration) = fixture();
  let (_, devices, rebuilt) =
    hardware::checkpoint(boot.clone(), &package, configuration, || Ok(())).unwrap();
  assert_eq!(rebuilt, configuration);
  assert!(devices[4].is_some());
  assert!(!std::path::Path::new(boot.disk.as_ref().unwrap()).exists());
  assert!(!std::path::Path::new(&boot.store).exists());
  assert!(machine::snapshot::checkpoint::load(&package, &devices, || Ok(())).is_err());
}

#[test]
fn changed_hardware_media_binding_and_revoked_policy_reject_shape() {
  let (_root, boot, package, configuration) = fixture();
  for fault in 0..5 {
    let mut config = boot.clone();
    let mut expected = configuration;
    match fault {
      0 => config.cpus = 1,
      1 => config.disk_id = "hopper00000000000002".into(),
      2 => config.boot_media = Some("/unopened-installer".into()),
      3 => expected[0] ^= 1,
      _ => config.disk = None,
    }
    assert!(hardware::checkpoint(config, &package, expected, || Ok(())).is_err());
  }
  let mut count = 0;
  assert!(hardware::checkpoint(boot, &package, configuration, || {
    count += 1;
    anyhow::ensure!(count < 5, "revoked during component read");
    Ok(())
  })
  .is_err());
}

#[test]
fn prototype_rejects_unbounded_corrupt_foreign_and_readonly_metadata() {
  use machine::devices::virtio::block::snapshot::record::prototype;
  for bytes in [
    disk_record(0, false),
    disk_record(u64::MAX, false),
    disk_record((2u64 << 40) / 512 + 1, false),
    disk_record(8, true),
  ] {
    assert!(prototype(&bytes, *b"hopper00000000000001").is_err());
  }
  let mut bytes = disk_record(8, false);
  bytes[40] ^= 1;
  assert!(prototype(&bytes, *b"hopper00000000000001").is_err());
  assert!(prototype(&disk_record(8, false), *b"hopper00000000000002").is_err());
}
