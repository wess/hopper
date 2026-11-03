#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use engine::machines::linux::tools;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

#[test]
#[ignore = "requires scripts/build/guest.sh and the independent system ISO reader"]
fn built_daemon_is_loaded_and_recovered_from_production_seed_media() {
  use base64::{engine::general_purpose::STANDARD, Engine};
  use engine::machines::linux::{provision, seed};
  let assets = tools::bundled().unwrap();
  let plan = provision::prepare(&uuid::Uuid::new_v4().to_string(), &provision::accounts()).unwrap();
  let plan = provision::with_tools(&plan, &assets.executable, &assets.notices).unwrap();
  let root = tempfile::tempdir().unwrap();
  let path = root.path().join("seed.iso");
  fs::write(&path, seed::image(&plan).unwrap()).unwrap();
  let result = std::process::Command::new("/usr/bin/bsdtar")
    .args(["-xOf"])
    .arg(path)
    .arg("user-data")
    .output()
    .unwrap();
  assert!(result.status.success());
  let document: serde_json::Value = serde_yaml::from_slice(&result.stdout).unwrap();
  let files = document["autoinstall"]["user-data"]["write_files"]
    .as_array()
    .unwrap();
  let file = files
    .iter()
    .find(|file| file["path"] == "/usr/libexec/hopperguest")
    .unwrap();
  assert_eq!(
    STANDARD.decode(file["content"].as_str().unwrap()).unwrap(),
    assets.executable
  );
  let notices = files
    .iter()
    .find(|file| file["path"] == "/usr/share/doc/hopperguest/licenses.html")
    .unwrap();
  assert_eq!(notices["permissions"], "0644");
  assert_eq!(
    STANDARD
      .decode(notices["content"].as_str().unwrap())
      .unwrap(),
    assets.notices
  );
  assert!(!files.iter().any(|file| file["path"] == "/run/hopper/boot"));
}

fn executable() -> Vec<u8> {
  let mut data = vec![0; 128];
  data[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
  for (offset, value) in [(16, 2u16), (18, 183), (52, 64), (54, 56), (56, 1)] {
    data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
  }
  for (offset, value) in [(20, 1u32), (64, 1), (68, 5)] {
    data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
  }
  for (offset, value) in [
    (24, 0x400078u64),
    (32, 64),
    (72, 120),
    (80, 0x400078),
    (96, 8),
    (104, 8),
  ] {
    data[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
  }
  data
}

fn package(root: &Path, binary: &[u8], unit: &[u8]) {
  fs::write(root.join("hopperguest"), binary).unwrap();
  fs::write(root.join("hopperguest.service"), unit).unwrap();
  let notices = b"<html>license notices</html>";
  fs::write(root.join("licenses.html"), notices).unwrap();
  let artifact =
    |data: &[u8]| json!({"size": data.len(), "sha256": format!("{:x}", Sha256::digest(data))});
  fs::write(
    root.join("manifest.json"),
    serde_json::to_vec(&json!({
      "version": 1, "target": "aarch64-unknown-linux-musl", "files": {
        "hopperguest": artifact(binary), "hopperguest.service": artifact(unit),
        "licenses.html": artifact(notices)
      }
    }))
    .unwrap(),
  )
  .unwrap();
}

#[test]
fn packaged_app_and_sidecar_resolve_only_their_own_resources() {
  for executable in [
    "/Applications/Hopper.app/Contents/MacOS/hopper",
    "/Applications/Hopper.app/Contents/MacOS/sidecars/hoppermcp",
  ] {
    assert_eq!(
      tools::directory(Path::new(executable)).unwrap(),
      Path::new("/Applications/Hopper.app/Contents/Resources/guesttools")
    );
  }
  assert!(tools::directory(Path::new("relative/hopper")).is_err());
  assert!(tools::load(Path::new("relative/resources")).is_err());
}

#[test]
fn exact_resource_inventory_integrity_and_current_service_are_required() {
  let root = tempfile::tempdir().unwrap();
  let binary = executable();
  let unit = include_bytes!("../../guest/assets/hopperguest.service");
  package(root.path(), &binary, unit);
  let assets = tools::load(root.path()).unwrap();
  assert_eq!(assets.executable, binary);
  assert_eq!(assets.notices, b"<html>license notices</html>");
  fs::write(root.path().join("hopperguest"), vec![0; 128]).unwrap();
  assert!(tools::load(root.path()).is_err());
  package(root.path(), &binary, b"old service");
  assert!(tools::load(root.path()).is_err());
  package(root.path(), &binary, unit);
  fs::write(root.path().join("extra"), b"extra").unwrap();
  assert!(tools::load(root.path()).is_err());
  fs::remove_file(root.path().join("extra")).unwrap();
  fs::remove_file(root.path().join("hopperguest")).unwrap();
  std::os::unix::fs::symlink("manifest.json", root.path().join("hopperguest")).unwrap();
  assert!(tools::load(root.path()).is_err());
}

#[test]
fn dynamic_wrong_architecture_unbounded_and_nonexecutable_elf_reject() {
  tools::validate(&executable()).unwrap();
  for (offset, value) in [
    (18, 62u64),
    (32, u64::MAX),
    (64, 2),
    (64, 3),
    (68, 4),
    (72, 129),
    (96, 9),
    (24, 0),
  ] {
    let mut data = executable();
    data[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    assert!(tools::validate(&data).is_err());
  }
  for bytes in [vec![], vec![0; (1 << 20) + 1], executable()[..100].to_vec()] {
    assert!(tools::validate(&bytes).is_err());
  }
}
