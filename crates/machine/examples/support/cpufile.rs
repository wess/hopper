use anyhow::ensure;
use machine::hypervisor::state::{self, wire};
use std::{
  fs::{File, OpenOptions},
  io::{Read, Write},
  os::unix::fs::{MetadataExt, OpenOptionsExt},
  path::Path,
};
use zeroize::Zeroizing;

fn private(root: &Path) -> anyhow::Result<()> {
  let metadata = std::fs::symlink_metadata(root)?;
  ensure!(
    metadata.is_dir()
      && metadata.uid() == unsafe { libc::geteuid() }
      && metadata.mode() & 0o777 == 0o700,
    "CPU diagnostic directory is not private"
  );
  Ok(())
}

pub fn save(root: &Path, state: &state::State) -> anyhow::Result<()> {
  private(root)?;
  let encoded = wire::encode(state)?;
  let mut file = OpenOptions::new()
    .write(true)
    .create_new(true)
    .mode(0o600)
    .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
    .open(root.join("cpu.hcs"))?;
  file.write_all(&encoded)?;
  file.sync_all()?;
  File::open(root)?.sync_all()?;
  Ok(())
}

pub fn open(root: &Path) -> anyhow::Result<state::State> {
  private(root)?;
  let file = OpenOptions::new()
    .read(true)
    .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
    .open(root.join("cpu.hcs"))?;
  let metadata = file.metadata()?;
  ensure!(
    metadata.is_file()
      && metadata.uid() == unsafe { libc::geteuid() }
      && metadata.nlink() == 1
      && metadata.mode() & 0o777 == 0o600
      && metadata.len() <= wire::MAX as u64,
    "CPU diagnostic record is not a private bounded file"
  );
  let mut encoded = Zeroizing::new(Vec::new());
  file
    .take((wire::MAX + 1) as u64)
    .read_to_end(&mut encoded)?;
  ensure!(
    encoded.len() as u64 == metadata.len(),
    "CPU diagnostic record changed while reading"
  );
  wire::decode(&encoded)
}
