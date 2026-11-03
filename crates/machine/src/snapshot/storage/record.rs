//! checksummed image records in caller-owned private staging directories.

use super::{checksum, clone, Image};
use anyhow::{ensure, Context};
use sha2::{Digest, Sha256};
use std::{
  fs::File,
  os::{
    fd::{AsRawFd, FromRawFd},
    unix::fs::{FileExt, MetadataExt},
  },
};

pub const SIZE: usize = 76;
const LIMIT: u64 = 2 * 1024u64.pow(4);

/// The directory is pinned by descriptor. Success is not publication of a VM snapshot.
/// On any failure the caller must discard the stage; existing names are never replaced.
pub fn save(
  image: &Image,
  directory: &File,
  mut authorize: impl FnMut() -> anyhow::Result<()>,
) -> anyhow::Result<[u8; SIZE]> {
  authorize()?;
  private(directory)?;
  ensure!(
    image.file.metadata()?.len() == image.size,
    "Captured image capacity changed"
  );
  if unsafe {
    super::fclonefileat(
      image.file.as_raw_fd(),
      directory.as_raw_fd(),
      c"image".as_ptr(),
      3,
    )
  } != 0
  {
    return Err(std::io::Error::last_os_error()).context("Stage snapshot image on APFS");
  }
  authorize()?;
  let staged = open(directory, image.size)?;
  let source = image.file.metadata()?;
  let copied = staged.metadata()?;
  ensure!(
    (copied.dev(), copied.ino()) != (source.dev(), source.ino()),
    "Snapshot staging reused source inode"
  );
  let digest = checksum::sparse(&staged, image.size, &mut authorize)?;
  staged.sync_all()?;
  directory.sync_all()?;
  authorize()?;
  let mut bytes = [0; SIZE];
  bytes[..4].copy_from_slice(b"HDS2");
  bytes[4..12].copy_from_slice(&image.size.to_le_bytes());
  bytes[12..44].copy_from_slice(&digest);
  let checksum = Sha256::digest(&bytes[..44]);
  bytes[44..].copy_from_slice(&checksum);
  Ok(bytes)
}

/// Clones into fresh private ownership before verification; source paths are not retained.
/// HDS1 scans all bytes; HDS2 skips filesystem holes. Authorization cannot bound OS syscall time.
pub fn load(
  directory: &File,
  bytes: &[u8],
  expected: u64,
  mut authorize: impl FnMut() -> anyhow::Result<()>,
) -> anyhow::Result<Image> {
  authorize()?;
  validate(bytes, expected)?;
  private(directory)?;
  let source = open(directory, expected)?;
  let (file, directory) = clone(&source, true)?;
  authorize()?;
  let digest = if &bytes[..4] == b"HDS1" {
    hash(&file, expected, &mut authorize)?
  } else {
    checksum::sparse(&file, expected, &mut authorize)?
  };
  ensure!(digest == bytes[12..44], "Snapshot image checksum mismatch");
  authorize()?;
  Ok(Image {
    file,
    _directory: directory,
    size: expected,
  })
}

pub fn validate(bytes: &[u8], expected: u64) -> anyhow::Result<()> {
  ensure!(
    bytes.len() == SIZE && matches!(&bytes[..4], b"HDS1" | b"HDS2"),
    "Unknown or malformed disk image record"
  );
  let size = u64::from_le_bytes(bytes[4..12].try_into()?);
  ensure!(
    size == expected && (1..=LIMIT).contains(&size),
    "Disk image record capacity mismatch"
  );
  ensure!(
    Sha256::digest(&bytes[..44])[..] == bytes[44..],
    "Disk image record checksum mismatch"
  );
  Ok(())
}

fn private(directory: &File) -> anyhow::Result<()> {
  let metadata = directory.metadata()?;
  ensure!(
    metadata.is_dir()
      && metadata.uid() == unsafe { libc::geteuid() }
      && metadata.mode() & 0o7777 == 0o700,
    "Snapshot staging directory is not private"
  );
  Ok(())
}

fn open(directory: &File, size: u64) -> anyhow::Result<File> {
  let fd = unsafe {
    libc::openat(
      directory.as_raw_fd(),
      c"image".as_ptr(),
      libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
    )
  };
  ensure!(
    fd >= 0,
    "Open staged snapshot image: {}",
    std::io::Error::last_os_error()
  );
  let file = unsafe { File::from_raw_fd(fd) };
  let metadata = file.metadata()?;
  ensure!(
    metadata.is_file()
      && metadata.len() == size
      && (1..=LIMIT).contains(&size)
      && metadata.uid() == unsafe { libc::geteuid() }
      && metadata.nlink() == 1
      && metadata.mode() & 0o7777 == 0o400,
    "Staged snapshot image is not private and bounded"
  );
  Ok(file)
}

fn hash(
  file: &File,
  size: u64,
  authorize: &mut impl FnMut() -> anyhow::Result<()>,
) -> anyhow::Result<[u8; 32]> {
  let mut digest = Sha256::new();
  let mut bytes = [0; 65536];
  let mut offset = 0;
  while offset < size {
    authorize()?;
    let count = (size - offset).min(bytes.len() as u64) as usize;
    file.read_exact_at(&mut bytes[..count], offset)?;
    digest.update(&bytes[..count]);
    offset += count as u64;
    authorize()?;
  }
  ensure!(
    file.metadata()?.len() == size,
    "Snapshot image changed during hashing"
  );
  authorize()?;
  Ok(digest.finalize().into())
}
