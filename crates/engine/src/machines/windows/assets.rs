//! Bundled preparation tools and guest drivers, independent of the desktop prototype.

use super::setup::{self, files};
use crate::machines::native::{assets, deployment::Tools};
use anyhow::{ensure, Context};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
  collections::{BTreeMap, BTreeSet},
  io::Read,
  path::{Component, Path, PathBuf},
};

const DRIVER_HASH: &str = "2f58eea024e0221a873032761dc3ed3262409cd80860b7b27d3d5a0f206616c9";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
  version: u32,
  driver_package: Package,
  files: BTreeMap<String, Artifact>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Package {
  url: String,
  sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Artifact {
  size: u64,
  sha256: String,
  executable: bool,
}

pub fn locate() -> anyhow::Result<Tools> {
  verify(&candidates(&std::env::current_exe()?)?)
}

pub fn candidates(exe: &Path) -> anyhow::Result<PathBuf> {
  let (_, firmware) = assets::candidates(exe)?;
  Ok(
    firmware
      .parent()
      .context("Missing native resources directory")?
      .join("windows"),
  )
}

pub fn verify(root: &Path) -> anyhow::Result<Tools> {
  ensure!(
    root.is_absolute(),
    "Windows assets directory must be absolute"
  );
  ensure!(
    std::fs::symlink_metadata(root)?.is_dir(),
    "Windows assets directory cannot be a symlink"
  );
  let file = assets::regular(&root.join("manifest.json"), 1024 * 1024)?;
  let manifest: Manifest = serde_json::from_reader(file.take(1024 * 1024 + 1))?;
  ensure!(
    manifest.version == 1 && (1..=256).contains(&manifest.files.len()),
    "Invalid Windows assets manifest"
  );
  ensure!(manifest.driver_package.sha256 == DRIVER_HASH && manifest.driver_package.url == "https://fedorapeople.org/groups/virt/virtio-win/direct-downloads/archive-virtio/virtio-win-0.1.302-1/virtio-win-0.1.302-1.noarch.rpm", "Unexpected guest driver package");
  let mut required: BTreeSet<String> = [
    "bin/wimlib-imagex",
    "bin/mkisofs",
    "licenses/virtio.txt",
    "licenses/packages.json",
  ]
  .into_iter()
  .map(str::to_owned)
  .collect();
  for (group, names) in files::DRIVERS {
    for name in *names {
      required.insert(format!("drivers/{group}/w11/ARM64/{name}"));
    }
  }
  ensure!(
    required
      .iter()
      .all(|name| manifest.files.contains_key(name)),
    "Incomplete Windows assets"
  );
  let mut actual = BTreeSet::new();
  let mut entries = 0;
  inventory(root, root, &mut actual, &mut entries, 0)?;
  actual.remove("manifest.json");
  ensure!(
    actual == manifest.files.keys().cloned().collect(),
    "Windows assets inventory mismatch"
  );
  for (name, artifact) in &manifest.files {
    let relative = Path::new(name);
    ensure!(
      name.is_ascii()
        && name
          .bytes()
          .all(|byte| byte.is_ascii_alphanumeric() || b"/._-+".contains(&byte))
        && relative
          .components()
          .all(|part| matches!(part, Component::Normal(_))),
      "Invalid Windows asset path"
    );
    let code = name.starts_with("bin/") || name.starts_with("lib/");
    ensure!(
      !name.starts_with("bin/") || matches!(name.as_str(), "bin/wimlib-imagex" | "bin/mkisofs"),
      "Unexpected Windows preparation executable"
    );
    ensure!(
      code || name.starts_with("drivers/") || name.starts_with("licenses/"),
      "Unexpected Windows asset location"
    );
    ensure!(
      artifact.executable == code,
      "Invalid Windows asset executable policy"
    );
    check(&root.join(relative), artifact)?;
  }
  for (group, names) in files::DRIVERS {
    for name in *names {
      let path = root
        .join("drivers")
        .join(group)
        .join("w11/ARM64")
        .join(name);
      let mut bytes = Vec::new();
      assets::regular(&path, 8 * 1024 * 1024)?
        .take(8 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
      ensure!(
        bytes.len() <= 8 * 1024 * 1024,
        "Driver file grew during verification"
      );
      files::validate(name, &bytes)?;
    }
  }
  Ok(Tools {
    media: setup::Tools {
      archive: "/usr/bin/tar".into(),
      wim: root.join("bin/wimlib-imagex"),
      image: root.join("bin/mkisofs"),
    },
    mount: "/usr/sbin/diskutil".into(),
    drivers: root.join("drivers"),
    license: root.join("licenses/virtio.txt"),
  })
}

fn inventory(
  root: &Path,
  folder: &Path,
  files: &mut BTreeSet<String>,
  entries: &mut usize,
  depth: usize,
) -> anyhow::Result<()> {
  ensure!(
    depth < 16,
    "Windows assets directory nesting exceeds its bound"
  );
  for entry in std::fs::read_dir(folder)? {
    *entries += 1;
    ensure!(
      *entries <= 1024,
      "Windows asset inventory exceeds its bound"
    );
    let path = entry?.path();
    let info = std::fs::symlink_metadata(&path)?;
    if info.is_dir() {
      inventory(root, &path, files, entries, depth + 1)?;
    } else {
      ensure!(
        info.is_file(),
        "Windows assets cannot contain symlinks or special files"
      );
      let name = path
        .strip_prefix(root)?
        .to_str()
        .context("Windows asset path is not UTF-8")?
        .to_owned();
      ensure!(
        files.len() < 257 && files.insert(name),
        "Too many Windows asset files"
      );
    }
  }
  Ok(())
}

fn check(path: &Path, artifact: &Artifact) -> anyhow::Result<()> {
  ensure!(
    (1..=64 * 1024 * 1024).contains(&artifact.size)
      && artifact.sha256.len() == 64
      && artifact.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()),
    "Invalid Windows asset checksum metadata"
  );
  let file = assets::regular(path, artifact.size)?;
  ensure!(
    file.metadata()?.len() == artifact.size,
    "Windows asset size mismatch"
  );
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    ensure!(
      !artifact.executable || file.metadata()?.permissions().mode() & 0o111 != 0,
      "Windows media tool is not executable"
    );
  }
  let mut file = file.take(artifact.size + 1);
  let mut hash = Sha256::new();
  let mut total = 0u64;
  let mut buffer = vec![0; 64 * 1024];
  loop {
    let read = file.read(&mut buffer)?;
    if read == 0 {
      break;
    }
    total += read as u64;
    hash.update(&buffer[..read]);
  }
  ensure!(
    total == artifact.size
      && format!("{:x}", hash.finalize()).eq_ignore_ascii_case(&artifact.sha256),
    "Windows asset checksum mismatch"
  );
  Ok(())
}
