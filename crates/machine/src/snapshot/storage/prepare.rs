use super::{extent, Image};
use anyhow::{ensure, Context};
use fs2::FileExt;
use std::{
  fs::{File, Permissions},
  os::{
    fd::{AsRawFd, FromRawFd},
    unix::fs::{FileExt as _, MetadataExt, PermissionsExt},
  },
};

/// A writable clone in a caller-owned restore stage; this does not select it for a VM.
pub struct Prepared {
  file: Option<File>,
  directory: File,
  source: (u64, u64),
  size: u64,
}

impl Prepared {
  /// Check the original capture, complete contents and current private namespace before adoption.
  pub fn validate(
    &self,
    image: &Image,
    mut authorize: impl FnMut() -> anyhow::Result<()>,
  ) -> anyhow::Result<()> {
    authorize()?;
    let source = image.file.metadata()?;
    ensure!(
      self.source == (source.dev(), source.ino())
        && self.size == image.size
        && source.len() == self.size,
      "Prepared disk belongs to another capture"
    );
    self.namespace()?;
    let mut actual = [0; 65536];
    let mut expected = [0; 65536];
    let mut offset = 0;
    while offset < self.size {
      authorize()?;
      offset = extent::next(&image.file, offset, self.size)?.min(extent::next(
        self.file(),
        offset,
        self.size,
      )?);
      authorize()?;
      if offset == self.size {
        break;
      }
      let count = (self.size - offset).min(65536) as usize;
      image.read(offset, &mut expected[..count])?;
      self.file().read_exact_at(&mut actual[..count], offset)?;
      ensure!(
        actual[..count] == expected[..count],
        "Prepared disk differs from captured contents"
      );
      offset += count as u64;
    }
    ensure!(
      image.file.metadata()?.len() == self.size,
      "Captured disk capacity changed during comparison"
    );
    self.namespace()?;
    authorize()
  }

  pub(crate) fn into_file(mut self) -> File {
    self.file.take().expect("owned prepared disk transfer")
  }

  fn namespace(&self) -> anyhow::Result<()> {
    private(&self.directory)?;
    let actual = self.file().metadata()?;
    let current = open(&self.directory, true)?.metadata()?;
    ensure!(
      (actual.dev(), actual.ino()) == (current.dev(), current.ino()) && actual.len() == self.size,
      "Prepared disk namespace was replaced before adoption"
    );
    Ok(())
  }

  pub fn file(&self) -> &File {
    self.file.as_ref().expect("retained prepared disk")
  }
}

impl Drop for Prepared {
  fn drop(&mut self) {
    // explicit unlock also ends ownership when a trusted caller retained a duplicated descriptor.
    if let Some(file) = &self.file {
      let _ = FileExt::unlock(file);
    }
  }
}

impl Image {
  /// Re-admit an unconsumed restore disk against the original immutable capture.
  /// Written generations cannot be admitted with an old CPU checkpoint.
  pub fn reopen_prepared(
    &self,
    directory: &File,
    mut authorize: impl FnMut() -> anyhow::Result<()>,
  ) -> anyhow::Result<Prepared> {
    authorize()?;
    private(directory)?;
    let source = self.file.metadata()?;
    let file = open(directory, true)?;
    let retained = directory.try_clone()?;
    file
      .try_lock_exclusive()
      .context("Restored disk already has an owner")?;
    let prepared = Prepared {
      file: Some(file),
      directory: retained,
      source: (source.dev(), source.ino()),
      size: self.size,
    };
    prepared.validate(self, &mut authorize)?;
    Ok(prepared)
  }

  /// Caller retains original authorization and operation ownership; failures preserve the stage.
  /// The fixed disk name is exclusive. APFS cloning is required; no full-copy fallback is used.
  pub fn prepare(
    &self,
    directory: &File,
    mut authorize: impl FnMut() -> anyhow::Result<()>,
  ) -> anyhow::Result<Prepared> {
    authorize()?;
    private(directory)?;
    let source = self.file.metadata()?;
    ensure!(
      source.is_file() && source.len() == self.size && source.nlink() == 1,
      "Captured restore source identity is invalid"
    );
    if unsafe {
      super::fclonefileat(
        self.file.as_raw_fd(),
        directory.as_raw_fd(),
        c"disk".as_ptr(),
        3,
      )
    } != 0
    {
      return Err(std::io::Error::last_os_error())
        .context("Prepare restored disk on APFS without replacement");
    }
    let cloned = open(directory, false)?;
    let copied = cloned.metadata()?;
    ensure!(
      copied.len() == self.size && (copied.dev(), copied.ino()) != (source.dev(), source.ino()),
      "Restored disk clone identity is invalid"
    );
    authorize()?;
    cloned.set_permissions(Permissions::from_mode(0o600))?;
    cloned.sync_all()?;
    let file = open(directory, true)?;
    let reopened = file.metadata()?;
    ensure!(
      (reopened.dev(), reopened.ino()) == (copied.dev(), copied.ino())
        && reopened.len() == self.size,
      "Restored disk changed during preparation"
    );
    let retained = directory.try_clone()?;
    file
      .try_lock_exclusive()
      .context("Restored disk already has an owner")?;
    let prepared = Prepared {
      file: Some(file),
      directory: retained,
      source: (source.dev(), source.ino()),
      size: self.size,
    };
    directory.sync_all()?;
    private(directory)?;
    let current = open(directory, false)?.metadata()?;
    ensure!(
      (current.dev(), current.ino()) == (copied.dev(), copied.ino()) && current.len() == self.size,
      "Restored disk namespace was replaced"
    );
    authorize()?;
    Ok(prepared)
  }
}

pub(super) fn private(directory: &File) -> anyhow::Result<()> {
  let metadata = directory.metadata()?;
  ensure!(
    metadata.is_dir()
      && metadata.uid() == unsafe { libc::geteuid() }
      && metadata.mode() & 0o7777 == 0o700,
    "Restore disk namespace must be private"
  );
  Ok(())
}

pub(super) fn open(directory: &File, writable: bool) -> anyhow::Result<File> {
  let fd = unsafe {
    libc::openat(
      directory.as_raw_fd(),
      c"disk".as_ptr(),
      (if writable {
        libc::O_RDWR
      } else {
        libc::O_RDONLY
      }) | libc::O_NOFOLLOW
        | libc::O_CLOEXEC
        | libc::O_NONBLOCK,
    )
  };
  ensure!(
    fd >= 0,
    "Open prepared disk: {}",
    std::io::Error::last_os_error()
  );
  let file = unsafe { File::from_raw_fd(fd) };
  let metadata = file.metadata()?;
  ensure!(
    metadata.is_file()
      && metadata.uid() == unsafe { libc::geteuid() }
      && metadata.nlink() == 1
      && (1..=2 * 1024u64.pow(4)).contains(&metadata.len())
      && (if writable {
        metadata.mode() & 0o7777 == 0o600
      } else {
        matches!(metadata.mode() & 0o7777, 0o400 | 0o600)
      }),
    "Prepared disk is not private and bounded"
  );
  Ok(file)
}
