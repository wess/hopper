#![cfg(unix)]

#[allow(dead_code)]
#[path = "setup/fixture.rs"]
mod source;

use engine::machines::windows::assets;
use sha2::{Digest, Sha256};
use std::{
  os::unix::fs::{symlink, PermissionsExt},
  path::{Path, PathBuf},
};

struct Fixture {
  source: source::Fixture,
  root: PathBuf,
}

impl Fixture {
  fn new() -> Self {
    let source = source::Fixture::new("normal");
    let root = source.root.path().join("windows");
    for folder in ["bin", "lib", "licenses"] {
      std::fs::create_dir_all(root.join(folder)).unwrap();
    }
    for name in ["wimlib-imagex", "mkisofs"] {
      let path = root.join("bin").join(name);
      std::fs::write(&path, b"synthetic native helper").unwrap();
      std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    std::fs::write(root.join("licenses/virtio.txt"), b"synthetic license").unwrap();
    std::fs::write(root.join("licenses/packages.json"), b"{}").unwrap();
    let input = source.input();
    fn copy(from: &Path, to: &Path) {
      std::fs::create_dir(to).unwrap();
      for entry in from.read_dir().unwrap() {
        let path = entry.unwrap().path();
        let target = to.join(path.file_name().unwrap());
        if path.is_dir() {
          copy(&path, &target);
        } else {
          std::fs::copy(path, target).unwrap();
        }
      }
    }
    copy(input.drivers, &root.join("drivers"));
    let fixture = Self { source, root };
    fixture.manifest();
    fixture
  }

  fn manifest(&self) {
    let mut files = serde_json::Map::new();
    fn visit(root: &Path, folder: &Path, files: &mut serde_json::Map<String, serde_json::Value>) {
      for entry in folder.read_dir().unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
          visit(root, &path, files);
        } else if path.file_name().unwrap() != "manifest.json" {
          let bytes = std::fs::read(&path).unwrap();
          let name = path
            .strip_prefix(root)
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
          let executable = name.starts_with("bin/") || name.starts_with("lib/");
          files.insert(name, serde_json::json!({"size": bytes.len(), "sha256": format!("{:x}", Sha256::digest(bytes)), "executable": executable}));
        }
      }
    }
    visit(&self.root, &self.root, &mut files);
    std::fs::write(self.root.join("manifest.json"), serde_json::to_vec(&serde_json::json!({
      "version": 1,
      "driverPackage": { "url": "https://fedorapeople.org/groups/virt/virtio-win/direct-downloads/archive-virtio/virtio-win-0.1.302-1/virtio-win-0.1.302-1.noarch.rpm", "sha256": "2f58eea024e0221a873032761dc3ed3262409cd80860b7b27d3d5a0f206616c9" },
      "files": files,
    })).unwrap()).unwrap();
  }
}

#[test]
fn native_windows_assets_are_coherent_for_app_and_sidecars() {
  let root = Path::new("/Applications/Hopper.app/Contents");
  for exe in [
    root.join("MacOS/hopper"),
    root.join("MacOS/sidecars/hoppermcp"),
  ] {
    assert_eq!(
      assets::candidates(&exe).unwrap(),
      root.join("Resources/windows")
    );
  }
  let f = Fixture::new();
  let tools = assets::verify(&f.root).unwrap();
  assert_eq!(tools.media.wim, f.root.join("bin/wimlib-imagex"));
  assert_eq!(tools.drivers, f.root.join("drivers"));
  assert_eq!(tools.mount, Path::new("/usr/sbin/diskutil"));
  assert!(f.source.root.path().exists());
}

#[test]
fn modified_missing_and_unlisted_assets_are_rejected() {
  let f = Fixture::new();
  std::fs::write(f.root.join("licenses/virtio.txt"), b"modified license").unwrap();
  assert!(assets::verify(&f.root).is_err());
  f.manifest();
  std::fs::remove_file(f.root.join("drivers/vioscsi/w11/ARM64/vioscsi.cat")).unwrap();
  f.manifest();
  assert!(assets::verify(&f.root).is_err());
  let f = Fixture::new();
  std::fs::write(f.root.join("extra"), b"unlisted data").unwrap();
  assert!(assets::verify(&f.root).is_err());
}

#[test]
fn helper_permissions_driver_architecture_and_manifest_bounds_are_checked() {
  let f = Fixture::new();
  let wim = f.root.join("bin/wimlib-imagex");
  std::fs::set_permissions(&wim, std::fs::Permissions::from_mode(0o644)).unwrap();
  assert!(assets::verify(&f.root).is_err());
  std::fs::set_permissions(&wim, std::fs::Permissions::from_mode(0o755)).unwrap();
  let driver = f.root.join("drivers/vioscsi/w11/ARM64/vioscsi.sys");
  let mut bytes = std::fs::read(&driver).unwrap();
  bytes[68..70].copy_from_slice(&0x8664u16.to_le_bytes());
  std::fs::write(driver, bytes).unwrap();
  f.manifest();
  assert!(assets::verify(&f.root).is_err());
  std::fs::write(f.root.join("manifest.json"), vec![b' '; 1024 * 1024 + 1]).unwrap();
  assert!(assets::verify(&f.root).is_err());
}

#[test]
fn symlinks_and_additional_executables_are_not_accepted() {
  let f = Fixture::new();
  let library = f.root.join("lib/alias.dylib");
  symlink(f.root.join("bin/mkisofs"), &library).unwrap();
  assert!(assets::verify(&f.root).is_err());
  std::fs::remove_file(library).unwrap();
  std::fs::write(f.root.join("bin/other"), b"unexpected executable").unwrap();
  f.manifest();
  assert!(assets::verify(&f.root).is_err());
}
