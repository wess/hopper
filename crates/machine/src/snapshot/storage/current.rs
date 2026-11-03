use super::prepare::{open, private};
use crate::devices::virtio::block;
use anyhow::{ensure, Context};
use fs2::FileExt;
use std::{fs::File, os::unix::fs::MetadataExt};

/// Mutable generation storage for fresh system boot, never a captured CPU checkpoint.
pub struct Current {
  file: Option<File>,
  directory: File,
  size: u64,
}

impl Current {
  pub fn open(
    directory: &File,
    mut authorize: impl FnMut() -> anyhow::Result<()>,
  ) -> anyhow::Result<Self> {
    authorize()?;
    private(directory)?;
    let file = open(directory, true)?;
    let size = file.metadata()?.len();
    ensure!(
      size.is_multiple_of(512),
      "Current disk is not sector aligned"
    );
    let directory = directory.try_clone()?;
    file
      .try_lock_exclusive()
      .context("Current generation disk is already owned")?;
    let current = Self {
      file: Some(file),
      directory,
      size,
    };
    current.check(&mut authorize)?;
    Ok(current)
  }

  pub fn file(&self) -> &File {
    self.file.as_ref().expect("owned current disk")
  }

  pub fn check(&self, mut authorize: impl FnMut() -> anyhow::Result<()>) -> anyhow::Result<()> {
    authorize()?;
    private(&self.directory)?;
    let original = self.file().metadata()?;
    let current = open(&self.directory, true)?.metadata()?;
    ensure!(
      original.dev() == current.dev()
        && original.ino() == current.ino()
        && original.len() == self.size
        && current.len() == self.size,
      "Current generation disk namespace or capacity changed"
    );
    authorize()
  }

  /// Fresh controller state; the caller independently validates the complete hardware fingerprint.
  pub fn disk(
    mut self,
    id: [u8; 20],
    mut authorize: impl FnMut() -> anyhow::Result<()>,
  ) -> anyhow::Result<block::Disk> {
    self.check(&mut authorize)?;
    let disk = block::attach(self.file().try_clone()?, false, id)?;
    authorize()?;
    // the controller now owns the lock; errors before transfer explicitly unlock every duplicate.
    drop(self.file.take());
    Ok(disk)
  }
}

impl Drop for Current {
  fn drop(&mut self) {
    if let Some(file) = &self.file {
      let _ = FileExt::unlock(file);
    }
  }
}
