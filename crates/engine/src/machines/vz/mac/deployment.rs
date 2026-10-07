use super::super::files;
use anyhow::{ensure, Context};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
  fs::File,
  io::Read,
  os::unix::fs::MetadataExt,
  path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
  Installing,
  Installed,
}

impl Phase {
  pub fn message(self) -> &'static str {
    match self {
      Self::Installing => "macOS installation requires recovery; its disk is preserved",
      Self::Installed => "macOS installation finished; desktop readiness is unverified",
    }
  }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
  version: u32,
  id: String,
  attempt: String,
  binding: String,
  phase: Phase,
}

pub struct Attempt {
  directory: PathBuf,
  id: String,
  attempt: String,
  binding: String,
}

fn binding(directory: &Path, id: &str) -> anyhow::Result<String> {
  files::directory(directory)?;
  let mut bytes = Vec::new();
  files::read(&directory.join("platform"), 512 << 10)?
    .take((512 << 10) + 1)
    .read_to_end(&mut bytes)?;
  ensure!(
    bytes.len() <= 512 << 10,
    "macOS platform grew beyond bounds"
  );
  let platform: super::platform::Platform = serde_json::from_slice(&bytes)?;
  ensure!(
    platform.version == 1 && platform.id == id,
    "macOS platform identity changed"
  );
  Ok(format!("{:x}", Sha256::digest(&bytes)))
}

fn record(directory: &Path, id: &str) -> anyhow::Result<Option<Record>> {
  files::directory(directory)?;
  let path = directory.join("deployment");
  match std::fs::symlink_metadata(&path) {
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
    Err(error) => return Err(error.into()),
    Ok(_) => {}
  }
  let record: Record = serde_json::from_reader(files::read(&path, 1024)?.take(1025))?;
  ensure!(
    record.version == 1 && record.id == id,
    "macOS deployment identity changed"
  );
  ensure!(
    uuid::Uuid::parse_str(&record.attempt)?.to_string() == record.attempt,
    "Invalid macOS installation attempt"
  );
  ensure!(
    record.binding == binding(directory, id)?,
    "macOS deployment platform changed; preserve its disk for recovery"
  );
  Ok(Some(record))
}

pub fn read(directory: &Path, id: &str) -> anyhow::Result<Option<Phase>> {
  Ok(record(directory, id)?.map(|record| record.phase))
}

fn save(directory: &Path, record: &Record) -> anyhow::Result<()> {
  files::directory(directory)?;
  let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
  serde_json::to_writer(&mut temporary, record)?;
  temporary.as_file().sync_all()?;
  temporary.persist(directory.join("deployment"))?;
  File::open(directory)?.sync_all()?;
  Ok(())
}

pub fn written(directory: &Path) -> anyhow::Result<bool> {
  Ok(
    files::read(&directory.join("disk"), 2048 << 30)?
      .metadata()?
      .blocks()
      > 0,
  )
}

pub fn begin(directory: &Path, id: &str) -> anyhow::Result<Attempt> {
  crate::machines::validate_id(id)?;
  ensure!(
    read(directory, id)?.is_none(),
    "Existing macOS installation requires recovery or system boot; its disk is preserved"
  );
  let binding = binding(directory, id)?;
  ensure!(
    !written(directory)?,
    "Written macOS disk requires recovery; it must not be reinstalled automatically"
  );
  let attempt = Attempt {
    directory: directory.into(),
    id: id.into(),
    attempt: model::new_uuid(),
    binding,
  };
  save(
    directory,
    &Record {
      version: 1,
      id: attempt.id.clone(),
      attempt: attempt.attempt.clone(),
      binding: attempt.binding.clone(),
      phase: Phase::Installing,
    },
  )?;
  Ok(attempt)
}

pub fn installed(attempt: &Attempt) -> anyhow::Result<()> {
  let mut current =
    record(&attempt.directory, &attempt.id)?.context("macOS installation intent disappeared")?;
  ensure!(
    current.attempt == attempt.attempt
      && current.binding == attempt.binding
      && current.phase == Phase::Installing,
    "macOS installation completion is stale"
  );
  files::read(&attempt.directory.join("disk"), 2048 << 30)?.sync_all()?;
  files::read(&attempt.directory.join("auxiliary/state"), 128 << 20)?.sync_all()?;
  File::open(attempt.directory.join("auxiliary"))?.sync_all()?;
  current.phase = Phase::Installed;
  save(&attempt.directory, &current)
}
