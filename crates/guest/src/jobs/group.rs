use anyhow::{ensure, Context};
use std::{
  ffi::CString,
  fs::File,
  io::{Read, Write},
  os::fd::{AsRawFd, FromRawFd},
  os::unix::{
    ffi::OsStrExt,
    fs::{MetadataExt, OpenOptionsExt},
  },
  path::Path,
};

pub(super) struct Group {
  root: File,
  directory: File,
  name: CString,
  pub admission: File,
}

pub(super) fn open(root: &Path) -> anyhow::Result<File> {
  ensure!(
    unsafe { libc::geteuid() } == 0,
    "Guest job supervision requires its root service"
  );
  ensure!(
    root.is_absolute() && root.canonicalize()? == root,
    "Guest cgroup root must be canonical and absolute"
  );
  let root = std::fs::OpenOptions::new()
    .read(true)
    .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY)
    .open(root)?;
  let info = root.metadata()?;
  ensure!(
    info.uid() == 0 && info.gid() == 0 && info.mode() & 0o777 == 0o700,
    "Guest job cgroup root must be root-owned and private"
  );
  let mut filesystem: libc::statfs = unsafe { std::mem::zeroed() };
  ensure!(
    unsafe { libc::fstatfs(root.as_raw_fd(), &mut filesystem) } == 0
      && filesystem.f_type == 0x63677270,
    "Guest jobs require cgroup v2"
  );
  ensure!(
    unsafe { libc::flock(root.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
    "Guest job supervision is already owned"
  );
  for (count, entry) in
    std::fs::read_dir(format!("/proc/self/fd/{}", root.as_raw_fd()))?.enumerate()
  {
    ensure!(count < 256, "Guest job cgroup inventory exceeds bounds");
    ensure!(
      !entry?.file_type()?.is_dir(),
      "Previous guest job groups require recovery"
    );
  }
  Ok(root)
}

fn child(directory: &File, name: &CString, flags: i32) -> anyhow::Result<File> {
  let fd = unsafe {
    libc::openat(
      directory.as_raw_fd(),
      name.as_ptr(),
      flags | libc::O_NOFOLLOW | libc::O_CLOEXEC,
    )
  };
  ensure!(
    fd >= 0,
    "Open guest cgroup member: {}",
    std::io::Error::last_os_error()
  );
  Ok(unsafe { File::from_raw_fd(fd) })
}

pub(super) fn create(root: &File, id: &str) -> anyhow::Result<Group> {
  let name = CString::new(id)?;
  ensure!(
    unsafe { libc::mkdirat(root.as_raw_fd(), name.as_ptr(), 0o700) } == 0,
    "Create guest job cgroup: {}",
    std::io::Error::last_os_error()
  );
  let result = (|| {
    let directory = child(root, &name, libc::O_RDONLY | libc::O_DIRECTORY)?;
    ensure!(
      unsafe { libc::fchmod(directory.as_raw_fd(), 0o700) } == 0,
      "Protect guest job cgroup"
    );
    for (name, limit) in [
      ("cgroup.max.descendants", b"64\n".as_slice()),
      ("cgroup.max.depth", b"8\n".as_slice()),
    ] {
      child(&directory, &CString::new(name)?, libc::O_WRONLY)?.write_all(limit)?;
    }
    let admission = child(&directory, &CString::new("cgroup.procs")?, libc::O_WRONLY)?;
    let group = Group {
      root: root.try_clone()?,
      directory,
      name: name.clone(),
      admission,
    };
    let _kill = child(
      &group.directory,
      &CString::new("cgroup.kill")?,
      libc::O_WRONLY,
    )
    .context("Guest kernel lacks process-tree cancellation")?;
    Ok(group)
  })();
  if result.is_err() {
    unsafe {
      libc::unlinkat(root.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR);
    }
  }
  result
}

impl Group {
  pub fn kill(&self) -> anyhow::Result<()> {
    let mut file = child(
      &self.directory,
      &CString::new("cgroup.kill")?,
      libc::O_WRONLY,
    )?;
    file.write_all(b"1\n").context("Cancel guest process tree")
  }

  pub fn empty(&self) -> anyhow::Result<bool> {
    let file = child(
      &self.directory,
      &CString::new("cgroup.events")?,
      libc::O_RDONLY,
    )?;
    let mut text = String::new();
    file.take(4097).read_to_string(&mut text)?;
    ensure!(text.len() <= 4096, "Guest cgroup events exceed bounds");
    ensure!(
      text.lines().any(|line| line.starts_with("populated ")),
      "Guest cgroup population is absent"
    );
    Ok(text.lines().any(|line| line == "populated 0"))
  }

  pub fn retire(&self) -> anyhow::Result<()> {
    ensure!(self.empty()?, "Guest process tree remains populated");
    remove_children(&self.directory, 0, &mut 0)?;
    remove(&self.root, &self.name, &self.directory)
  }
}

fn remove_children(directory: &File, depth: usize, count: &mut usize) -> anyhow::Result<()> {
  ensure!(depth <= 8, "Guest cgroup cleanup depth exceeds bounds");
  for (entries, entry) in
    std::fs::read_dir(format!("/proc/self/fd/{}", directory.as_raw_fd()))?.enumerate()
  {
    ensure!(
      entries < 256,
      "Guest cgroup cleanup inventory exceeds bounds"
    );
    let entry = entry?;
    if !entry.file_type()?.is_dir() {
      continue;
    }
    *count += 1;
    ensure!(*count <= 64, "Guest cgroup cleanup count exceeds bounds");
    let name = CString::new(entry.file_name().as_bytes())?;
    let child = child(directory, &name, libc::O_RDONLY | libc::O_DIRECTORY)?;
    remove_children(&child, depth + 1, count)?;
    remove(directory, &name, &child)?;
  }
  Ok(())
}

fn remove(root: &File, name: &CString, directory: &File) -> anyhow::Result<()> {
  let mut current: libc::stat = unsafe { std::mem::zeroed() };
  let info = directory.metadata()?;
  ensure!(
    unsafe {
      libc::fstatat(
        root.as_raw_fd(),
        name.as_ptr(),
        &mut current,
        libc::AT_SYMLINK_NOFOLLOW,
      )
    } == 0
      && current.st_dev == info.dev()
      && current.st_ino == info.ino(),
    "Guest cgroup identity changed during cleanup"
  );
  ensure!(
    unsafe { libc::unlinkat(root.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR) } == 0,
    "Remove guest cgroup: {}",
    std::io::Error::last_os_error()
  );
  Ok(())
}

impl Drop for Group {
  fn drop(&mut self) {
    let _ = self.kill();
    let _ = self.retire();
  }
}
