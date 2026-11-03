mod journal;
mod root;
use root::Root;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod restore;
mod snapshot;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub(crate) use restore::matches;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub use restore::{from_snapshot, from_snapshot_at};

use anyhow::{ensure, Context};
use fs2::FileExt;
use std::{
  fs::File,
  io::{Seek, SeekFrom, Write},
  path::Path,
};

pub const SIZE: usize = 0x4000000;
const COMPACT: u64 = 4 * 1024 * 1024;

pub struct Variables {
  root: Root,
  _lock: File,
  log: File,
  bank: Vec<u8>,
  poisoned: bool,
  sequence: u64,
}

fn guard(root: &Root) -> anyhow::Result<File> {
  let lock = match root.file("lock", true) {
    Ok(file) => file,
    Err(error)
      if error
        .downcast_ref::<std::io::Error>()
        .is_some_and(|error| error.kind() == std::io::ErrorKind::AlreadyExists) =>
    {
      root.file("lock", false)?
    }
    Err(error) => return Err(error),
  };
  lock
    .try_lock_exclusive()
    .context("Another runtime owns this variable store")?;
  Ok(lock)
}

pub fn create(root: &Path, template: &[u8]) -> anyhow::Result<Variables> {
  ensure!(
    !template.is_empty() && template.len() <= SIZE,
    "Invalid variable template size"
  );
  create_inner(Root::create(root)?, template)
}

fn create_inner(root: Root, template: &[u8]) -> anyhow::Result<Variables> {
  let lock = guard(&root)?;
  let mut bank = vec![0xff; SIZE];
  bank[..template.len()].copy_from_slice(template);
  snapshot::write(&root, &bank, 0)?;
  let log = root.file("journal", true)?;
  log.sync_all()?;
  root.sync()?;
  Ok(Variables {
    root,
    _lock: lock,
    log,
    bank,
    poisoned: false,
    sequence: 0,
  })
}

pub fn open(root: &Path) -> anyhow::Result<Variables> {
  open_inner(Root::open(root)?)
}

fn open_inner(root: Root) -> anyhow::Result<Variables> {
  let lock = guard(&root)?;
  let (mut bank, mut sequence) = snapshot::read(&root)?;
  let saved = sequence;
  let mut log = root.file("journal", false)?;
  let retained = journal::replay(&mut log, &mut bank, &mut sequence)?;
  if retained < log.metadata()?.len() {
    log.set_len(retained)?;
    log.sync_all()?;
  }
  if retained > 0 {
    if sequence != saved {
      snapshot::write(&root, &bank, sequence)?;
    }
    log.set_len(0)?;
    log.sync_all()?;
  }
  log.seek(SeekFrom::End(0))?;
  Ok(Variables {
    root,
    _lock: lock,
    log,
    bank,
    poisoned: false,
    sequence,
  })
}

pub fn bytes(store: &Variables) -> &[u8] {
  &store.bank
}

/// persist confirmed NOR changes before guest execution can acknowledge them.
pub fn commit(store: &mut Variables, offset: usize, bytes: &[u8]) -> anyhow::Result<()> {
  ensure!(
    !store.poisoned,
    "Variable store requires recovery after an I/O failure"
  );
  journal::validate(offset, bytes.len())?;
  let result = (|| -> anyhow::Result<()> {
    let sequence = store
      .sequence
      .checked_add(1)
      .context("Variable sequence exhausted")?;
    let record = journal::record(sequence, offset, bytes)?;
    store.log.write_all(&record)?;
    store.log.sync_all()?;
    store.bank[offset..offset + bytes.len()].copy_from_slice(bytes);
    store.sequence = sequence;
    if store.log.metadata()?.len() >= COMPACT {
      snapshot::write(&store.root, &store.bank, store.sequence)?;
      // the snapshot sequence makes replay of a pre-compaction journal harmless.
      store.log.set_len(0)?;
      store.log.sync_all()?;
      store.log.seek(SeekFrom::Start(0))?;
    }
    Ok(())
  })();
  if result.is_err() {
    store.poisoned = true;
  }
  result.context("Persist guest variable change")
}

/// Exclusively create the fixed variables child of a caller-owned private stage.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub fn create_at(parent: &File, template: &[u8]) -> anyhow::Result<Variables> {
  ensure!(
    !template.is_empty() && template.len() <= SIZE,
    "Invalid variable template size"
  );
  create_inner(Root::child(parent, true)?, template)
}

/// Open and retain the fixed variables child through a pinned private parent.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub fn open_at(parent: &File) -> anyhow::Result<Variables> {
  open_inner(Root::child(parent, false)?)
}

impl Drop for Variables {
  fn drop(&mut self) {
    let _ = FileExt::unlock(&self._lock);
  }
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub(crate) fn directory(store: &Variables) -> anyhow::Result<File> {
  Ok(store.root.directory.try_clone()?)
}
