//! A complete installer and its source catalogue publish together under the media lease.

use super::super::{catalog, setup::files};
use crate::machines::native::assets;
use anyhow::{ensure, Context};
use serde::{Deserialize, Serialize};
use std::{
  io::{Read, Write},
  path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
  version: u32,
  origin: String,
  catalogue_sha256: String,
  source_sha1: String,
  source_size: u64,
  source_url: String,
  image_sha256: String,
  image_size: u64,
}

pub fn directory(path: &Path) -> anyhow::Result<()> {
  if !path.try_exists()? {
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
      use std::os::unix::fs::DirBuilderExt;
      builder.mode(0o700);
    }
    match builder.create(path) {
      Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
      result => result?,
    }
  }
  ensure!(
    std::fs::symlink_metadata(path)?.is_dir(),
    "Media directory must not be a symlink"
  );
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    ensure!(
      std::fs::metadata(path)?.permissions().mode() & 0o077 == 0,
      "Media directory must be private"
    );
  }
  Ok(())
}

fn read(path: &Path, limit: u64) -> anyhow::Result<Vec<u8>> {
  let file = assets::regular(path, limit)?;
  private(&file)?;
  let mut bytes = Vec::new();
  file.take(limit + 1).read_to_end(&mut bytes)?;
  ensure!(
    bytes.len() as u64 <= limit,
    "Media metadata grew beyond its bound"
  );
  Ok(bytes)
}

fn private(file: &std::fs::File) -> anyhow::Result<()> {
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    ensure!(
      file.metadata()?.permissions().mode() & 0o077 == 0,
      "Media file must be private"
    );
  }
  Ok(())
}

fn write(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
  let mut options = std::fs::OpenOptions::new();
  options.write(true).create_new(true);
  #[cfg(unix)]
  {
    use std::os::unix::fs::OpenOptionsExt;
    options.mode(0o600);
  }
  let mut file = options.open(path)?;
  file.write_all(bytes)?;
  file.sync_all()?;
  Ok(())
}

pub async fn verify(folder: &Path, archive: &Path) -> anyhow::Result<PathBuf> {
  ensure!(folder.is_absolute(), "Media cache path must be absolute");
  ensure!(
    std::fs::symlink_metadata(folder)?.is_dir(),
    "Media cache must be a directory"
  );
  directory(folder)?;
  let mut names = std::collections::BTreeSet::new();
  for entry in std::fs::read_dir(folder)? {
    let entry = entry?;
    ensure!(
      names.len() < 3 && entry.file_type()?.is_file(),
      "Unexpected cached media entry"
    );
    names.insert(entry.file_name());
  }
  ensure!(
    names
      == ["installer.iso", "manifest.json", "catalogue.cab"]
        .into_iter()
        .map(Into::into)
        .collect(),
    "Incomplete cached media"
  );
  let manifest: Manifest =
    serde_json::from_slice(&read(&folder.join("manifest.json"), 64 * 1024)?)?;
  ensure!(
    manifest.version == 1 && manifest.origin == catalog::ENDPOINT,
    "Invalid media provenance"
  );
  let selection = catalog::decode(
    &read(&folder.join("catalogue.cab"), 4 * 1024 * 1024)?,
    archive,
  )
  .await?;
  ensure!(
    manifest.catalogue_sha256 == selection.catalogue_sha256
      && manifest.source_sha1 == selection.media.sha1.to_ascii_lowercase()
      && manifest.source_size == selection.media.size
      && manifest.source_url == selection.media.file_path,
    "Cached media source does not match its catalogue"
  );
  let iso = folder.join("installer.iso");
  let file = assets::regular(&iso, 12 * 1024 * 1024 * 1024)?;
  private(&file)?;
  ensure!(
    file.metadata()?.len() == manifest.image_size,
    "Cached installer size mismatch"
  );
  ensure!(
    manifest.image_sha256 == files::digest(&iso, 12 * 1024 * 1024 * 1024).await?,
    "Cached installer checksum mismatch"
  );
  Ok(iso)
}

/// Call only after conversion of an ESD verified against the supplied official selection.
pub async fn publish(
  stage: &Path,
  destination: &Path,
  selection: &catalog::Selection,
  archive: &Path,
) -> anyhow::Result<PathBuf> {
  ensure!(
    stage.is_absolute() && destination.is_absolute(),
    "Media publication paths must be absolute"
  );
  match std::fs::symlink_metadata(destination) {
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
    Err(error) => return Err(error.into()),
    Ok(_) => anyhow::bail!("Cached installer already exists; preserve it for recovery"),
  }
  directory(stage)?;
  let iso = stage.join("installer.iso");
  let file = assets::regular(&iso, 12 * 1024 * 1024 * 1024)?;
  private(&file)?;
  let manifest = Manifest {
    version: 1,
    origin: catalog::ENDPOINT.into(),
    catalogue_sha256: selection.catalogue_sha256.clone(),
    source_sha1: selection.media.sha1.to_ascii_lowercase(),
    source_size: selection.media.size,
    source_url: selection.media.file_path.clone(),
    image_size: file.metadata()?.len(),
    image_sha256: files::digest(&iso, 12 * 1024 * 1024 * 1024).await?,
  };
  file.sync_all()?;
  write(&stage.join("catalogue.cab"), &selection.catalogue)?;
  write(
    &stage.join("manifest.json"),
    &serde_json::to_vec(&manifest)?,
  )?;
  verify(stage, archive).await?;
  std::fs::File::open(stage)?.sync_all()?;
  std::fs::rename(stage, destination)?;
  std::fs::File::open(destination.parent().context("Missing cache parent")?)?.sync_all()?;
  Ok(destination.join("installer.iso"))
}
