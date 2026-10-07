use super::Source;
use crate::machines::vz::files;
use anyhow::{ensure, Context};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
  fs::{File, OpenOptions},
  io::{Read, Write},
  os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
  path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Receipt {
  pub version: u32,
  pub source: Source,
  pub sha256: Option<String>,
}

pub(super) struct Cache {
  pub directory: PathBuf,
  pub partial: PathBuf,
  pub complete: PathBuf,
  pub receipt: Receipt,
  pub _lock: File,
}

pub(super) fn open(root: &Path, source: &Source) -> anyhow::Result<Cache> {
  ensure!(root.is_absolute(), "Restore cache must be absolute");
  files::directory(root)?;
  let key = format!("{:x}", Sha256::digest(serde_json::to_vec(source)?));
  let directory = root.join(key);
  match std::fs::DirBuilder::new().mode(0o700).create(&directory) {
    Ok(()) => {}
    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
    Err(error) => return Err(error.into()),
  }
  files::directory(&directory)?;
  let lock = open_file(&directory.join("lock"), true)?;
  lock
    .try_lock_exclusive()
    .context("Another VM is acquiring this macOS restore image")?;
  let path = directory.join("receipt");
  let receipt = match std::fs::symlink_metadata(&path) {
    Ok(_) => {
      let file = files::read(&path, 16384)?;
      serde_json::from_reader::<_, Receipt>(file.take(16385))?
    }
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Receipt {
      version: 1,
      source: source.clone(),
      sha256: None,
    },
    Err(error) => return Err(error.into()),
  };
  ensure!(
    receipt.version == 1
      && receipt.source == *source
      && receipt
        .sha256
        .as_ref()
        .is_none_or(|hash| hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())),
    "Restore cache receipt does not match its download identity"
  );
  let cache = Cache {
    partial: directory.join("restore.part"),
    complete: directory.join("restore.ipsw"),
    directory,
    receipt,
    _lock: lock,
  };
  cache.save()?;
  Ok(cache)
}

pub(super) fn open_file(path: &Path, write: bool) -> anyhow::Result<File> {
  let file = OpenOptions::new()
    .read(true)
    .write(write)
    .create(write)
    .truncate(false)
    .mode(0o600)
    .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
    .open(path)?;
  let info = file.metadata()?;
  ensure!(
    info.is_file() && info.uid() == unsafe { libc::geteuid() } && info.mode() & 0o077 == 0,
    "Restore cache file must be private and owned"
  );
  Ok(file)
}

impl Cache {
  pub fn save(&self) -> anyhow::Result<()> {
    let mut stage = tempfile::NamedTempFile::new_in(&self.directory)?;
    stage.write_all(&serde_json::to_vec(&self.receipt)?)?;
    stage.as_file().sync_all()?;
    stage.persist(self.directory.join("receipt"))?;
    File::open(&self.directory)?.sync_all()?;
    Ok(())
  }

  pub fn publish(&self) -> anyhow::Result<()> {
    std::fs::hard_link(&self.partial, &self.complete)?;
    File::open(&self.directory)?.sync_all()?;
    std::fs::remove_file(&self.partial)?;
    File::open(&self.directory)?.sync_all()?;
    Ok(())
  }
}
