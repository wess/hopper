use super::fs;
use anyhow::ensure;
use sha2::{Digest, Sha256};
use std::{
  fs::File,
  io::{Read, Write},
  os::fd::AsRawFd,
};

pub(super) fn unused(directory: &File) -> anyhow::Result<()> {
  let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
  let result = unsafe {
    libc::fstatat(
      directory.as_raw_fd(),
      c"attempt.hga".as_ptr(),
      metadata.as_mut_ptr(),
      libc::AT_SYMLINK_NOFOLLOW,
    )
  };
  if result == 0 {
    anyhow::bail!("Restore generation was attempted or requires recovery");
  }
  let error = std::io::Error::last_os_error();
  ensure!(
    error.raw_os_error() == Some(libc::ENOENT),
    "Inspect restore attempt: {error}"
  );
  Ok(())
}

pub(super) fn write(directory: &File, receipt: &[u8]) -> anyhow::Result<Vec<u8>> {
  unused(directory)?;
  let mut nonce = [0; 16];
  unsafe { libc::arc4random_buf(nonce.as_mut_ptr().cast(), nonce.len()) };
  let mut bytes = Vec::from(b"HGA1");
  bytes.extend_from_slice(&Sha256::digest(receipt));
  bytes.extend_from_slice(&nonce);
  bytes.extend_from_slice(&Sha256::digest(&bytes));
  let mut file = fs::create(directory, c"attempt.hga")?;
  // any partial or unsealed marker also blocks replay; errors never remove it.
  file.write_all(&bytes)?;
  fs::seal(&file)?;
  directory.sync_all()?;
  check(directory, &bytes)?;
  Ok(bytes)
}

pub(super) fn check(directory: &File, expected: &[u8]) -> anyhow::Result<()> {
  let mut bytes = Vec::new();
  fs::open(directory, c"attempt.hga", 84)?
    .take(85)
    .read_to_end(&mut bytes)?;
  ensure!(
    bytes == expected,
    "Restore attempt changed before execution handoff"
  );
  Ok(())
}

pub(super) fn executed(directory: &File, receipt: &[u8]) -> anyhow::Result<Vec<u8>> {
  let mut bytes = Vec::new();
  fs::open(directory, c"attempt.hga", 84)?
    .take(85)
    .read_to_end(&mut bytes)?;
  ensure!(
    bytes.len() == 84
      && &bytes[..4] == b"HGA1"
      && bytes[4..36] == Sha256::digest(receipt)[..]
      && bytes[52..] == Sha256::digest(&bytes[..52])[..],
    "Invalid current generation attempt binding"
  );
  Ok(bytes)
}
