use anyhow::{ensure, Context};
#[cfg(unix)]
use std::os::unix::fs::DirBuilderExt;
#[cfg(all(unix, not(all(target_os = "macos", target_arch = "aarch64"))))]
use std::os::unix::fs::PermissionsExt;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use std::{
  ffi::CString,
  os::{
    fd::{AsRawFd, FromRawFd},
    unix::fs::{MetadataExt, OpenOptionsExt},
  },
};
use std::{
  fs::{File, OpenOptions},
  path::Path,
};

pub(super) struct Root {
  #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
  pub directory: File,
  #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
  path: std::path::PathBuf,
}

impl Root {
  pub fn create(path: &Path) -> anyhow::Result<Self> {
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    builder.mode(0o700);
    builder
      .create(path)
      .context("Create new private variable store")?;
    Self::open(path)
  }

  pub fn open(path: &Path) -> anyhow::Result<Self> {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
      let directory = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)?;
      Self::pinned(directory)
    }
    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    {
      private(path, true)?;
      Ok(Self { path: path.into() })
    }
  }

  #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
  pub fn child(parent: &File, create: bool) -> anyhow::Result<Self> {
    Self::private(parent)?;
    if create {
      ensure!(
        unsafe { libc::mkdirat(parent.as_raw_fd(), c"variables".as_ptr(), 0o700) } == 0,
        "Create restore variable directory: {}",
        std::io::Error::last_os_error()
      );
    }
    let fd = unsafe {
      libc::openat(
        parent.as_raw_fd(),
        c"variables".as_ptr(),
        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
      )
    };
    ensure!(
      fd >= 0,
      "Open restore variable directory: {}",
      std::io::Error::last_os_error()
    );
    Self::pinned(unsafe { File::from_raw_fd(fd) })
  }

  #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
  fn pinned(directory: File) -> anyhow::Result<Self> {
    Self::private(&directory)?;
    Ok(Self { directory })
  }

  #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
  fn private(directory: &File) -> anyhow::Result<()> {
    let metadata = directory.metadata()?;
    ensure!(
      metadata.is_dir()
        && metadata.uid() == unsafe { libc::geteuid() }
        && metadata.mode() & 0o7777 == 0o700,
      "Variable store directory is not private"
    );
    Ok(())
  }

  pub fn file(&self, name: &str, create: bool) -> anyhow::Result<File> {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
      Self::private(&self.directory)?;
      let name = CString::new(name)?;
      let fd = unsafe {
        libc::openat(
          self.directory.as_raw_fd(),
          name.as_ptr(),
          libc::O_RDWR
            | libc::O_NOFOLLOW
            | libc::O_CLOEXEC
            | libc::O_NONBLOCK
            | if create {
              libc::O_CREAT | libc::O_EXCL
            } else {
              0
            },
          0o600,
        )
      };
      if fd < 0 {
        return Err(std::io::Error::last_os_error()).context("Open private variable store file");
      }
      let file = unsafe { File::from_raw_fd(fd) };
      let metadata = file.metadata()?;
      ensure!(
        metadata.is_file()
          && metadata.uid() == unsafe { libc::geteuid() }
          && metadata.mode() & 0o7777 == 0o600
          && metadata.nlink() == 1,
        "Variable store file is not private"
      );
      Ok(file)
    }
    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    {
      let path = self.path.join(name);
      if !create {
        private(&path, false)?;
      }
      let mut options = OpenOptions::new();
      options.read(true).write(true).create_new(create);
      #[cfg(unix)]
      {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
      }
      options
        .open(path)
        .context("Open private variable store file")
    }
  }

  pub fn sync(&self) -> anyhow::Result<()> {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    self.directory.sync_all()?;
    #[cfg(all(unix, not(all(target_os = "macos", target_arch = "aarch64"))))]
    File::open(&self.path)?.sync_all()?;
    Ok(())
  }

  pub fn replace(&self, from: &str, to: &str) -> anyhow::Result<()> {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
      let from = CString::new(from)?;
      let to = CString::new(to)?;
      ensure!(
        unsafe {
          libc::renameat(
            self.directory.as_raw_fd(),
            from.as_ptr(),
            self.directory.as_raw_fd(),
            to.as_ptr(),
          )
        } == 0,
        "Publish variable bank: {}",
        std::io::Error::last_os_error()
      );
      Ok(())
    }
    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    {
      Ok(std::fs::rename(self.path.join(from), self.path.join(to))?)
    }
  }

  pub fn remove(&self, name: &str) {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    if let Ok(name) = CString::new(name) {
      unsafe { libc::unlinkat(self.directory.as_raw_fd(), name.as_ptr(), 0) };
    }
    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    {
      let _ = std::fs::remove_file(self.path.join(name));
    }
  }
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn private(path: &Path, directory: bool) -> anyhow::Result<()> {
  let metadata = std::fs::symlink_metadata(path)?;
  ensure!(
    if directory {
      metadata.is_dir()
    } else {
      metadata.is_file()
    },
    "Invalid variable store path"
  );
  #[cfg(unix)]
  ensure!(
    metadata.permissions().mode() & 0o777 == if directory { 0o700 } else { 0o600 },
    "Variable store must have private permissions"
  );
  Ok(())
}
