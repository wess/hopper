use super::{iso, kernel};
use crate::machines::native::assets;
use anyhow::ensure;
use machine::vz::queue::Check;
use std::{
  ffi::CString,
  fs::{File, OpenOptions},
  io::{Read, Seek, SeekFrom, Write},
  os::{
    fd::AsRawFd,
    unix::fs::{OpenOptionsExt, PermissionsExt},
  },
  path::Path,
};

pub const CONFIGURATION: &str = "set timeout=0\nset default=0\nmenuentry 'Install Ubuntu' {\n  set gfxpayload=keep\n  linux /casper/vmlinuz boot=casper autoinstall ds=nocloud console=hvc0 ---\n  initrd /casper/initrd\n}\n";

pub fn stage(media: &Path, parent: &Path, check: &Check) -> anyhow::Result<tempfile::TempDir> {
  check()?;
  let mut source = assets::regular(media, 16 << 30)?;
  let compressed = iso::read(&mut source, "casper/vmlinuz", kernel::MAXIMUM, check)?;
  drop(kernel::decode(&compressed, check)?);
  iso::locate(&mut source, "casper/initrd", 256 << 20, check)?;
  let sources = iso::read(&mut source, "casper/install-sources.yaml", 64 << 10, check)?;
  let sources: serde_yaml::Value = serde_yaml::from_slice(&sources)?;
  ensure!(
    sources
      .as_sequence()
      .is_some_and(|items| items.iter().any(|item| {
        matches!(
          item["id"].as_str(),
          Some("ubuntu-desktop" | "ubuntu-desktop-minimal")
        )
      })),
    "Choose Ubuntu Desktop ARM64 installation media"
  );
  let original = iso::read(&mut source, "boot/grub/grub.cfg", 64 << 10, check)?;
  let configuration = std::str::from_utf8(&original)?;
  ensure!(
    configuration.contains("/casper/vmlinuz")
      && configuration.contains("/casper/initrd")
      && CONFIGURATION.len() <= original.len(),
    "Unsupported Ubuntu installer boot configuration"
  );
  let (offset, size) = iso::locate(&mut source, "boot/grub/grub.cfg", 64 << 10, check)?;
  let directory = tempfile::Builder::new()
    .prefix("installer")
    .tempdir_in(parent)?;
  std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
  let destination = directory.path().join("installer");
  let parent = File::open(directory.path())?;
  let name = CString::new("installer")?;
  check()?;
  if unsafe { libc::fclonefileat(source.as_raw_fd(), parent.as_raw_fd(), name.as_ptr(), 0) } != 0 {
    let error = std::io::Error::last_os_error();
    ensure!(
      matches!(
        error.raw_os_error(),
        Some(libc::EXDEV | libc::ENOTSUP | libc::EINVAL)
      ),
      "Clone Ubuntu installer: {error}"
    );
    ensure!(
      fs2::available_space(directory.path())? >= source.metadata()?.len() + (64 << 20),
      "Staging this installer needs more free disk space"
    );
    let mut output = OpenOptions::new()
      .create_new(true)
      .write(true)
      .mode(0o600)
      .open(&destination)?;
    source.seek(SeekFrom::Start(0))?;
    let mut chunk = vec![0; 1 << 20];
    loop {
      check()?;
      let count = source.read(&mut chunk)?;
      if count == 0 {
        break;
      }
      output.write_all(&chunk[..count])?;
    }
    output.sync_all()?;
  }
  check()?;
  std::fs::set_permissions(&destination, std::fs::Permissions::from_mode(0o600))?;
  let mut output = OpenOptions::new()
    .read(true)
    .write(true)
    .custom_flags(libc::O_NOFOLLOW)
    .open(&destination)?;
  let mut padded = vec![b' '; size];
  padded[..CONFIGURATION.len()].copy_from_slice(CONFIGURATION.as_bytes());
  output.seek(SeekFrom::Start(offset))?;
  output.write_all(&padded)?;
  output.sync_all()?;
  ensure!(
    iso::read(&mut output, "boot/grub/grub.cfg", 64 << 10, check)? == padded,
    "Staged boot configuration changed"
  );
  check()?;
  Ok(directory)
}
