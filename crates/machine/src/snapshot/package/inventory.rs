use super::{description, fs, Description, Manifest, MAX};
use crate::snapshot::manifest::{Binding, Layout};
use anyhow::ensure;
use std::{ffi::CStr, fs::File, io::Read, os::fd::AsRawFd};

/// Manifest admission only; component decoding is still required before restore.
pub struct Summary {
  pub binding: Binding,
  pub layout: Layout,
  pub description: Option<Description>,
}

struct Directory(*mut libc::DIR);

impl Drop for Directory {
  fn drop(&mut self) {
    unsafe { libc::closedir(self.0) };
  }
}

/// Enumerate a pinned private namespace without following package or manifest symlinks.
/// Unknown entries are preserved. Damaged published entries reject the listing, never disappear.
pub fn inventory(
  parent: &File,
  machine: [u8; 16],
  mut authorize: impl FnMut() -> anyhow::Result<()>,
) -> anyhow::Result<Vec<Summary>> {
  authorize()?;
  ensure!(machine != [0; 16], "Snapshot machine identity is unset");
  fs::private(parent)?;
  // a new open description prevents one listing from moving another listing's cursor.
  let fd = unsafe {
    libc::openat(
      parent.as_raw_fd(),
      c".".as_ptr(),
      libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
    )
  };
  ensure!(
    fd >= 0,
    "Open snapshot listing: {}",
    std::io::Error::last_os_error()
  );
  let directory = unsafe { libc::fdopendir(fd) };
  if directory.is_null() {
    let error = std::io::Error::last_os_error();
    unsafe { libc::close(fd) };
    return Err(error.into());
  }
  let directory = Directory(directory);
  let mut found = Vec::new();
  for count in 0.. {
    authorize()?;
    unsafe { *libc::__error() = 0 };
    let entry = unsafe { libc::readdir(directory.0) };
    if entry.is_null() {
      ensure!(
        unsafe { *libc::__error() } == 0,
        "Read snapshot listing: {}",
        std::io::Error::last_os_error()
      );
      break;
    }
    ensure!(count < 4096, "Snapshot namespace exceeds its listing bound");
    let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
    let bytes = name.to_bytes();
    if bytes.len() != 32
      || !bytes
        .iter()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
    {
      continue;
    }
    let mut snapshot = [0; 16];
    for (index, pair) in bytes.as_chunks::<2>().0.iter().enumerate() {
      let digit = |byte: u8| {
        if byte <= b'9' {
          byte - b'0'
        } else {
          byte - b'a' + 10
        }
      };
      snapshot[index] = digit(pair[0]) * 16 + digit(pair[1]);
    }
    let pinned = fs::directory(parent, name)?;
    let mut bytes = Vec::new();
    fs::open(&pinned, c"manifest.hsm", MAX as u64)?
      .take(MAX as u64 + 1)
      .read_to_end(&mut bytes)?;
    let manifest = Manifest::inspect(&bytes)?;
    let binding = manifest.binding();
    ensure!(
      binding.machine == machine && binding.snapshot == snapshot,
      "Snapshot listing identity mismatch"
    );
    let description = description::read(&pinned, &manifest)?;
    fs::same(parent, name, &pinned)?;
    authorize()?;
    found.push(Summary {
      binding,
      layout: manifest.layout(),
      description,
    });
  }
  fs::private(parent)?;
  authorize()?;
  found.sort_by_key(|summary| summary.binding.snapshot);
  Ok(found)
}
