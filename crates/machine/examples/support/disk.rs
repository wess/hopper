use anyhow::Context;
use machine::devices::virtio::{block, pci};
use std::{fs::OpenOptions, path::Path};

pub fn create(path: &Path) -> anyhow::Result<pci::Device> {
  let file = OpenOptions::new()
    .read(true)
    .write(true)
    .create_new(true)
    .open(path)
    .with_context(|| format!("Create disposable target {}", path.display()))?;
  file.set_len(64 * 1024 * 1024)?;
  file.sync_all()?;
  pci::create(block::attach(file, false, *b"hopper target probe ")?)
}
