//! Coherent worker/firmware lookup; a packaged app never falls back to a development helper.

use anyhow::{ensure, Context};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
  fs::File,
  io::Read,
  path::{Path, PathBuf},
};

#[derive(Clone)]
pub struct Assets {
  pub(super) worker: PathBuf,
  pub(super) firmware: PathBuf,
  pub(super) variables: PathBuf,
}

#[derive(Deserialize)]
struct Manifest {
  size: u64,
  sha256: String,
  variables: Artifact,
}

#[derive(Deserialize)]
struct Artifact {
  size: u64,
  sha256: String,
}

pub fn worker(assets: &Assets) -> &Path {
  &assets.worker
}

pub fn locate() -> anyhow::Result<Assets> {
  let exe = std::env::current_exe()?;
  let (worker, firmware) = candidates(&exe)?;
  verify(&worker, &firmware)
}

pub fn candidates(exe: &Path) -> anyhow::Result<(PathBuf, PathBuf)> {
  ensure!(exe.is_absolute(), "Native executable path must be absolute");
  if let Some(contents) = exe.ancestors().find(|path| {
    path.file_name().is_some_and(|name| name == "Contents")
      && path.parent().is_some_and(|parent| {
        parent
          .extension()
          .is_some_and(|extension| extension == "app")
      })
  }) {
    return Ok((
      contents.join("MacOS/sidecars/hoppervm"),
      contents.join("Resources/firmware"),
    ));
  }
  let mut folder = exe
    .parent()
    .context("Native executable has no parent directory")?;
  if folder
    .file_name()
    .is_some_and(|name| name == "examples" || name == "deps")
  {
    folder = folder
      .parent()
      .context("Native target directory is missing")?;
  }
  Ok((
    folder.join("hoppervm"),
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../native/build/firmware"),
  ))
}

pub fn verify(worker: &Path, folder: &Path) -> anyhow::Result<Assets> {
  ensure!(
    worker.is_absolute() && folder.is_absolute(),
    "Native assets need absolute paths"
  );
  let helper = regular(worker, 64 * 1024 * 1024)?;
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    ensure!(
      helper.metadata()?.permissions().mode() & 0o111 != 0,
      "Native worker is not executable"
    );
  }
  let manifest = regular(&folder.join("manifest.json"), 1024 * 1024)?;
  let manifest: Manifest = serde_json::from_reader(manifest.take(1024 * 1024 + 1))?;
  let firmware = folder.join("windows.fd");
  let variables = folder.join("variables.fd");
  check(&firmware, manifest.size, &manifest.sha256)?;
  check(
    &variables,
    manifest.variables.size,
    &manifest.variables.sha256,
  )?;
  Ok(Assets {
    worker: worker.to_owned(),
    firmware,
    variables,
  })
}

pub(crate) fn regular(path: &Path, limit: u64) -> anyhow::Result<File> {
  let metadata = std::fs::symlink_metadata(path)
    .with_context(|| format!("Missing native asset: {}", path.display()))?;
  ensure!(
    metadata.is_file() && (1..=limit).contains(&metadata.len()),
    "Invalid native asset: {}",
    path.display()
  );
  let mut options = std::fs::OpenOptions::new();
  options.read(true);
  #[cfg(unix)]
  {
    use std::os::unix::fs::OpenOptionsExt;
    options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
  }
  let file = options.open(path)?;
  ensure!(
    file.metadata()?.is_file() && (1..=limit).contains(&file.metadata()?.len()),
    "Native asset changed while opening"
  );
  Ok(file)
}

fn check(path: &Path, size: u64, expected: &str) -> anyhow::Result<()> {
  ensure!(
    (1..=64 * 1024 * 1024).contains(&size),
    "Invalid firmware artifact size"
  );
  ensure!(
    expected.len() == 64 && expected.bytes().all(|byte| byte.is_ascii_hexdigit()),
    "Invalid firmware checksum"
  );
  let mut file = regular(path, size)?;
  ensure!(
    file.metadata()?.len() == size,
    "Firmware artifact size mismatch"
  );
  let mut hash = Sha256::new();
  let mut buffer = [0; 64 * 1024];
  let mut total = 0u64;
  loop {
    let read = file.read(&mut buffer)?;
    if read == 0 {
      break;
    }
    total += read as u64;
    ensure!(total <= size, "Firmware artifact grew while verifying");
    hash.update(&buffer[..read]);
  }
  ensure!(
    total == size && format!("{:x}", hash.finalize()).eq_ignore_ascii_case(expected),
    "Firmware artifact checksum mismatch"
  );
  Ok(())
}
