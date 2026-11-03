use anyhow::{ensure, Context};
use std::{
  ffi::CString,
  fs::{File, OpenOptions},
  io::Write,
  os::{
    fd::AsRawFd,
    unix::{
      ffi::OsStrExt,
      fs::{MetadataExt, OpenOptionsExt},
    },
  },
  path::{Path, PathBuf},
};

pub(super) struct Writer {
  file: File,
  directory: File,
  parent: PathBuf,
  identity: (u64, u64),
  name: CString,
  expected: u64,
  written: u64,
}

pub(super) fn prepare(path: &Path, expected: u64) -> anyhow::Result<Writer> {
  let raw = path.as_os_str().as_bytes();
  let name = raw
    .rsplit(|byte| *byte == b'/')
    .next()
    .context("Missing guest file name")?;
  ensure!(
    path.is_absolute()
      && raw.len() <= 4096
      && !raw.contains(&0)
      && !name.is_empty()
      && name.len() <= 255
      && name != b"."
      && name != b".."
      && expected <= model::guest::files::UPLOAD_LIMIT,
    "Invalid guest file creation"
  );
  let parent = path
    .parent()
    .context("Missing guest parent directory")?
    .to_owned();
  let directory = OpenOptions::new()
    .read(true)
    .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
    .open(&parent)?;
  let original = directory.metadata()?;
  let mut filesystem: libc::statfs = unsafe { std::mem::zeroed() };
  ensure!(
    unsafe { libc::fstatfs(directory.as_raw_fd(), &mut filesystem) } == 0,
    "Inspect guest destination filesystem"
  );
  ensure!(
    ![
      i128::from(libc::PROC_SUPER_MAGIC),
      i128::from(libc::SYSFS_MAGIC)
    ]
    .contains(&i128::from(filesystem.f_type)),
    "Guest files cannot be created in proc/sysfs"
  );
  // no pathname exists until a complete private inode is linked without replacement.
  let file = OpenOptions::new()
    .read(true)
    .write(true)
    .mode(0o600)
    .custom_flags(libc::O_TMPFILE | libc::O_CLOEXEC)
    .open(format!("/proc/self/fd/{}", directory.as_raw_fd()))
    .context("Guest destination requires anonymous temporary-file support")?;
  let metadata = file.metadata()?;
  ensure!(
    metadata.is_file()
      && metadata.nlink() == 0
      && metadata.len() == 0
      && metadata.uid() == unsafe { libc::geteuid() }
      && metadata.mode() & 0o7777 == 0o600,
    "Guest temporary file must be private and owned by the executing user"
  );
  Ok(Writer {
    file,
    directory,
    parent,
    name: CString::new(name)?,
    identity: (original.dev(), original.ino()),
    expected,
    written: 0,
  })
}

impl Writer {
  pub(super) fn append(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
    ensure!(
      bytes.len() <= super::CHUNK && bytes.len() as u64 <= self.expected - self.written,
      "Guest file chunk exceeds transfer bounds"
    );
    self.file.write_all(bytes)?;
    self.written += bytes.len() as u64;
    Ok(())
  }

  pub(super) fn publish(self) -> anyhow::Result<model::guest::files::Receipt> {
    ensure!(
      self.written == self.expected && self.file.metadata()?.len() == self.expected,
      "Guest file is incomplete"
    );
    self.file.sync_all()?;
    let current = std::fs::metadata(&self.parent)?;
    ensure!(
      (current.dev(), current.ino()) == self.identity,
      "Guest destination directory changed before publication"
    );
    let source = CString::new(format!("/proc/self/fd/{}", self.file.as_raw_fd()))?;
    let result = unsafe {
      libc::linkat(
        libc::AT_FDCWD,
        source.as_ptr(),
        self.directory.as_raw_fd(),
        self.name.as_ptr(),
        libc::AT_SYMLINK_FOLLOW,
      )
    };
    ensure!(
      result == 0,
      "Publish new guest file without replacement: {}",
      std::io::Error::last_os_error()
    );
    self
      .directory
      .sync_all()
      .context("Guest file was published but directory synchronization failed")?;
    Ok(model::guest::files::Receipt {
      stamp: super::measure(self.file.metadata()?)?,
      offset: 0,
      length: self.written as usize,
      eof: true,
    })
  }
}
