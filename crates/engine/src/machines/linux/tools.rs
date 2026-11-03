use anyhow::{ensure, Context};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
  collections::BTreeMap,
  io::Read,
  path::{Path, PathBuf},
};

const UNIT: &[u8] = include_bytes!("../../../../guest/assets/hopperguest.service");

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
  version: u32,
  target: String,
  files: BTreeMap<String, Artifact>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Artifact {
  size: u64,
  sha256: String,
}

pub fn directory(executable: &Path) -> anyhow::Result<PathBuf> {
  ensure!(
    executable.is_absolute(),
    "Guest resource executable path must be absolute"
  );
  if let Some(contents) = executable.ancestors().find(|path| {
    path.file_name().is_some_and(|name| name == "Contents")
      && path
        .parent()
        .is_some_and(|parent| parent.extension().is_some_and(|ext| ext == "app"))
  }) {
    return Ok(contents.join("Resources/guesttools"));
  }
  Ok(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../native/build/guesttools"))
}

pub struct Assets {
  pub executable: Vec<u8>,
  pub notices: Vec<u8>,
}

pub fn bundled() -> anyhow::Result<Assets> {
  load(&directory(&std::env::current_exe()?)?)
}

pub fn load(root: &Path) -> anyhow::Result<Assets> {
  ensure!(root.is_absolute(), "Guest resource path must be absolute");
  ensure!(
    std::fs::symlink_metadata(root)?.is_dir(),
    "Invalid guest resource directory"
  );
  let mut names = std::fs::read_dir(root)?
    .map(|entry| entry.map(|entry| entry.file_name()))
    .collect::<std::io::Result<Vec<_>>>()?;
  names.sort();
  ensure!(
    names
      == [
        "hopperguest",
        "hopperguest.service",
        "licenses.html",
        "manifest.json"
      ]
      .map(std::ffi::OsString::from),
    "Incomplete or unexpected guest resources"
  );
  let manifest: Manifest = serde_json::from_slice(&read(&root.join("manifest.json"), 4096)?)?;
  ensure!(
    manifest.version == 1
      && manifest.target == "aarch64-unknown-linux-musl"
      && manifest.files.keys().map(String::as_str).eq([
        "hopperguest",
        "hopperguest.service",
        "licenses.html"
      ]),
    "Invalid guest resource manifest"
  );
  let mut binary = None;
  let mut notices = None;
  for (name, artifact) in manifest.files {
    let limit = match name.as_str() {
      "hopperguest" => 1 << 20,
      "licenses.html" => 4 << 20,
      _ => 4096,
    };
    ensure!(
      (1..=limit).contains(&artifact.size)
        && artifact.sha256.len() == 64
        && artifact.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()),
      "Invalid guest resource metadata"
    );
    let data = read(&root.join(&name), limit)?;
    ensure!(
      data.len() as u64 == artifact.size
        && format!("{:x}", Sha256::digest(&data)) == artifact.sha256.to_ascii_lowercase(),
      "Guest resource checksum mismatch"
    );
    if name == "hopperguest" {
      validate(&data)?;
      binary = Some(data);
    } else if name == "hopperguest.service" {
      ensure!(data == UNIT, "Guest service does not match this build");
    } else {
      ensure!(
        !data.contains(&0) && std::str::from_utf8(&data).is_ok(),
        "Invalid guest license notices"
      );
      notices = Some(data);
    }
  }
  Ok(Assets {
    executable: binary.context("Guest executable is missing")?,
    notices: notices.context("Guest license notices are missing")?,
  })
}

fn read(path: &Path, limit: u64) -> anyhow::Result<Vec<u8>> {
  use std::os::unix::fs::OpenOptionsExt;
  let file = std::fs::OpenOptions::new()
    .read(true)
    .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
    .open(path)?;
  let metadata = file.metadata()?;
  ensure!(
    metadata.is_file() && (1..=limit).contains(&metadata.len()),
    "Invalid guest resource file"
  );
  let mut data = Vec::with_capacity(metadata.len() as usize);
  file.take(limit + 1).read_to_end(&mut data)?;
  ensure!(
    data.len() as u64 == metadata.len(),
    "Guest resource changed while reading"
  );
  Ok(data)
}

pub fn validate(data: &[u8]) -> anyhow::Result<()> {
  ensure!(
    (64..=1 << 20).contains(&data.len()) && &data[..7] == b"\x7fELF\x02\x01\x01",
    "Guest tools require a bounded ARM64 ELF executable"
  );
  let word = |offset| u16::from_le_bytes(data[offset..offset + 2].try_into().unwrap());
  let quad = |offset| u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap());
  let entry = quad(24);
  let offset = quad(32);
  let count = word(56) as u64;
  ensure!(
    matches!(word(16), 2 | 3)
      && word(18) == 183
      && data[20..24] == 1u32.to_le_bytes()
      && word(52) == 64
      && word(54) == 56
      && (1..=128).contains(&count)
      && offset >= 64
      && offset <= data.len() as u64
      && count * 56 <= data.len() as u64 - offset,
    "Invalid ARM64 ELF program headers"
  );
  let mut executable = false;
  for index in 0..count {
    let position = (offset + index * 56) as usize;
    let kind = u32::from_le_bytes(data[position..position + 4].try_into().unwrap());
    let flags = u32::from_le_bytes(data[position + 4..position + 8].try_into().unwrap());
    let start = quad(position + 8);
    let address = quad(position + 16);
    let size = quad(position + 32);
    let memory = quad(position + 40);
    ensure!(
      !matches!(kind, 2 | 3)
        && start <= data.len() as u64
        && size <= data.len() as u64 - start
        && size <= memory,
      "Guest executable must have bounded static segments"
    );
    executable |= kind == 1 && flags & 1 != 0 && entry >= address && entry - address < size;
  }
  ensure!(
    executable,
    "Guest executable entry is outside executable segments"
  );
  Ok(())
}
