use anyhow::ensure;
#[cfg(target_os = "linux")]
use model::guest::files::Browse;
use model::guest::files::{valid_name, Entry, Kind, Listing, Stamp};
use std::{
  collections::BTreeMap,
  fs::{File, OpenOptions},
  os::{
    fd::AsRawFd,
    unix::{
      ffi::OsStrExt,
      fs::{MetadataExt, OpenOptionsExt},
    },
  },
  path::Path,
  sync::Mutex,
};

const SCAN_LIMIT: usize = 65536;

fn stamp(file: &File) -> anyhow::Result<Stamp> {
  let metadata = file.metadata()?;
  ensure!(metadata.is_dir(), "Guest browsing requires a directory");
  let stamp = Stamp {
    device: metadata.dev(),
    inode: metadata.ino(),
    size: metadata.len(),
    modified: (metadata.mtime(), metadata.mtime_nsec()),
    changed: (metadata.ctime(), metadata.ctime_nsec()),
  };
  ensure!(stamp.valid(), "Guest directory metadata exceeds bounds");
  Ok(stamp)
}

pub struct Directory {
  file: File,
  stamp: Stamp,
  gate: Mutex<()>,
}

pub fn open(path: &Path) -> anyhow::Result<Directory> {
  let bytes = path.as_os_str().as_bytes();
  ensure!(
    path.is_absolute()
      && bytes.len() <= 4096
      && !bytes.contains(&0)
      && (bytes == b"/" || !bytes.ends_with(b"/")),
    "Guest directory path must be bounded and absolute"
  );
  let file = OpenOptions::new()
    .read(true)
    .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
    .open(path)?;
  #[cfg(target_os = "linux")]
  {
    let mut filesystem: libc::statfs = unsafe { std::mem::zeroed() };
    ensure!(
      unsafe { libc::fstatfs(file.as_raw_fd(), &mut filesystem) } == 0,
      "Inspect guest directory filesystem"
    );
    ensure!(
      ![
        i128::from(libc::PROC_SUPER_MAGIC),
        i128::from(libc::SYSFS_MAGIC)
      ]
      .contains(&i128::from(filesystem.f_type)),
      "Guest browsing cannot inspect process or kernel pseudo-files"
    );
  }
  Ok(Directory {
    stamp: stamp(&file)?,
    file,
    gate: Mutex::new(()),
  })
}

impl Directory {
  pub fn read(
    &self,
    limit: usize,
    after: Option<&[u8]>,
    expected: Option<Stamp>,
  ) -> anyhow::Result<Listing> {
    let _gate = self.gate.lock().unwrap();
    ensure!(
      (1..=model::guest::files::DIRECTORY_LIMIT).contains(&limit) && after.is_none_or(valid_name),
      "Invalid guest directory page"
    );
    ensure!(
      expected.is_none_or(|expected| expected == self.stamp) && stamp(&self.file)? == self.stamp,
      "Guest directory changed during browsing"
    );
    let fd = unsafe { libc::fcntl(self.file.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 3) };
    ensure!(
      fd >= 0,
      "Duplicate guest directory: {}",
      std::io::Error::last_os_error()
    );
    let directory = unsafe { libc::fdopendir(fd) };
    if directory.is_null() {
      let error = std::io::Error::last_os_error();
      unsafe {
        libc::close(fd);
      }
      return Err(error.into());
    }
    struct Handle(*mut libc::DIR);
    impl Drop for Handle {
      fn drop(&mut self) {
        unsafe {
          libc::closedir(self.0);
        }
      }
    }
    let directory = Handle(directory);
    unsafe {
      libc::rewinddir(directory.0);
    }
    let mut selected = BTreeMap::new();
    let mut index = 0;
    loop {
      #[cfg(target_os = "macos")]
      let errno = unsafe { libc::__error() };
      #[cfg(not(target_os = "macos"))]
      let errno = unsafe { libc::__errno_location() };
      unsafe {
        *errno = 0;
      }
      let entry = unsafe { libc::readdir(directory.0) };
      if entry.is_null() {
        let error = unsafe { *errno };
        ensure!(
          error == 0,
          "Read guest directory: {}",
          std::io::Error::from_raw_os_error(error)
        );
        break;
      }
      let name = unsafe { std::ffi::CStr::from_ptr((*entry).d_name.as_ptr()) }
        .to_bytes()
        .to_vec();
      if name == b"." || name == b".." {
        continue;
      }
      ensure!(index < SCAN_LIMIT, "Guest directory exceeds scan limit");
      index += 1;
      ensure!(valid_name(&name), "Invalid guest directory entry name");
      if after.is_some_and(|after| name.as_slice() <= after) {
        continue;
      }
      if selected.len() == limit + 1
        && selected
          .last_key_value()
          .is_some_and(|(last, _)| &name >= last)
      {
        continue;
      }
      let kind = match unsafe { (*entry).d_type } {
        libc::DT_REG => Kind::File,
        libc::DT_DIR => Kind::Directory,
        libc::DT_LNK => Kind::Symlink,
        libc::DT_UNKNOWN => Kind::Unknown,
        _ => Kind::Other,
      };
      selected.insert(name, kind);
      if selected.len() > limit + 1 {
        selected.pop_last();
      }
    }
    ensure!(
      stamp(&self.file)? == self.stamp,
      "Guest directory changed during browsing"
    );
    let more = selected.len() > limit;
    if more {
      selected.pop_last();
    }
    let next = if more {
      selected.last_key_value().map(|(name, _)| name.clone())
    } else {
      None
    };
    Ok(Listing {
      stamp: self.stamp,
      entries: selected
        .into_iter()
        .map(|(name, kind)| Entry { name, kind })
        .collect(),
      next,
    })
  }
}

#[cfg(target_os = "linux")]
pub fn command(args: &[std::ffi::OsString]) -> anyhow::Result<()> {
  use std::io::Write;
  ensure!(args.len() == 5, "Invalid guest directory arguments");
  let text = |index: usize| {
    args[index]
      .to_str()
      .ok_or_else(|| anyhow::anyhow!("Invalid guest directory argument"))
  };
  ensure!(
    text(3)?.len() <= 2048 && text(4)?.len() <= 512,
    "Guest directory arguments exceed bounds"
  );
  let request = Browse {
    path: text(1)?.into(),
    limit: text(2)?.parse()?,
    after: serde_json::from_str(text(3)?)?,
    stamp: serde_json::from_str(text(4)?)?,
    role: model::guest::Role::Normal,
  };
  ensure!(request.command().is_some(), "Invalid guest directory page");
  let result =
    open(Path::new(&args[1]))?.read(request.limit, request.after.as_deref(), request.stamp)?;
  let bytes = serde_json::to_vec(&result)?;
  ensure!(
    bytes.len() < 64 << 10,
    "Guest directory receipt exceeds output bound"
  );
  std::io::stdout().lock().write_all(&bytes)?;
  Ok(())
}
