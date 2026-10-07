use anyhow::ensure;

pub fn header(bytes: &[u8]) -> anyhow::Result<()> {
  ensure!(bytes.len() >= 64, "ARM64 kernel header is truncated");
  ensure!(&bytes[56..60] == b"ARM\x64", "VZ direct boot requires an uncompressed ARM64 Image; compressed EFI kernels need the EFI boot path");
  ensure!(
    bytes[24] & 1 == 0,
    "VZ requires a little-endian ARM64 kernel"
  );
  Ok(())
}

#[cfg(target_os = "macos")]
pub(super) fn file(path: &std::path::Path) -> anyhow::Result<()> {
  use std::{io::Read, os::unix::fs::OpenOptionsExt};
  let mut file = std::fs::OpenOptions::new()
    .read(true)
    .custom_flags(libc::O_NOFOLLOW)
    .open(path)?;
  let metadata = file.metadata()?;
  ensure!(
    metadata.is_file() && (64..=512 * 1024 * 1024).contains(&metadata.len()),
    "VZ kernel file exceeds bounds"
  );
  let mut bytes = [0; 64];
  file.read_exact(&mut bytes)?;
  header(&bytes)
}
