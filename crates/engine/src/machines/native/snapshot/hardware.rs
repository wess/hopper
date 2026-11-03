use super::super::config;
use crate::machines::Machines;
use anyhow::{ensure, Context};
use machine::snapshot::manifest::Layout;
use model::native::Boot;
use std::{
  fs::{File, OpenOptions},
  os::{
    fd::{AsRawFd, FromRawFd},
    unix::fs::{MetadataExt, OpenOptionsExt},
  },
  path::{Path, PathBuf},
  sync::Arc,
};

#[derive(Clone)]
pub(crate) struct Hardware {
  root: Arc<File>,
  store: PathBuf,
  pub layout: Layout,
  pub boot: Boot,
  pub helper: PathBuf,
}

impl Hardware {
  pub fn namespace_stamp(&self) -> anyhow::Result<(u64, u64)> {
    private(&self.root)?;
    let metadata = self.root.metadata()?;
    Ok((metadata.dev(), metadata.ino()))
  }

  pub fn capture(boot: &Boot, helper: &Path) -> anyhow::Result<Self> {
    let store = PathBuf::from(&boot.store);
    let root = open(store.parent().context("VM store has no parent")?)?;
    Ok(Self {
      root: Arc::new(root),
      store,
      boot: boot.clone(),
      helper: helper.to_owned(),
      layout: Layout {
        cpus: boot.cpus.try_into()?,
        banks: [
          (0, 0x4000000),
          (0x4000000, 0x4000000),
          (machine::platform::RAM, boot.memory),
        ],
        devices: [
          if boot.boot_media.is_some() { 4 } else { 0 },
          2,
          3,
          3,
          if boot.disk.is_some() { 1 } else { 0 },
          if boot.installer.is_some() { 4 } else { 0 },
          5,
        ],
      },
    })
  }

  pub fn namespace(&self, manager: &Machines, id: &str) -> anyhow::Result<File> {
    let paths = config::paths(&manager.root, id)?;
    ensure!(
      self.store == paths.variables,
      "Snapshot VM store does not match its saved identity"
    );
    self.layout.records()?;
    let current = open(&paths.root)?.metadata()?;
    let original = self.root.metadata()?;
    ensure!(
      (current.dev(), current.ino()) == (original.dev(), original.ino()),
      "Snapshot VM namespace was replaced"
    );
    private(&self.root)?;
    let result = unsafe { libc::mkdirat(self.root.as_raw_fd(), c"snapshots".as_ptr(), 0o700) };
    ensure!(
      result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EEXIST),
      "Create snapshot namespace: {}",
      std::io::Error::last_os_error()
    );
    let fd = unsafe {
      libc::openat(
        self.root.as_raw_fd(),
        c"snapshots".as_ptr(),
        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
      )
    };
    ensure!(
      fd >= 0,
      "Open snapshot namespace: {}",
      std::io::Error::last_os_error()
    );
    let file = unsafe { File::from_raw_fd(fd) };
    private(&file)?;
    Ok(file)
  }
}

fn open(path: &Path) -> anyhow::Result<File> {
  ensure!(path.is_absolute(), "VM namespace must be absolute");
  let file = OpenOptions::new()
    .read(true)
    .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
    .open(path)?;
  private(&file)?;
  Ok(file)
}

fn private(file: &File) -> anyhow::Result<()> {
  let meta = file.metadata()?;
  ensure!(
    meta.is_dir() && meta.uid() == unsafe { libc::geteuid() } && meta.mode() & 0o7777 == 0o700,
    "VM snapshot namespace is not private"
  );
  Ok(())
}
