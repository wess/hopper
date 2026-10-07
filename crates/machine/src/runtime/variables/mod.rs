mod journal;
mod snapshot;

use anyhow::{ensure, Context};
use fs2::FileExt;
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::{
  fs::{File, OpenOptions},
  io::{Seek, SeekFrom, Write},
  path::{Path, PathBuf},
};

pub const SIZE: usize = 0x4000000;
const COMPACT: u64 = 4 * 1024 * 1024;

pub struct Variables {
  root: PathBuf,
  _lock: File,
  log: File,
  bank: Vec<u8>,
  poisoned: bool,
  sequence: u64,
}

fn private(path: &Path, directory: bool) -> anyhow::Result<()> {
  let metadata = std::fs::symlink_metadata(path)?;
  ensure!(
    if directory {
      metadata.is_dir()
    } else {
      metadata.is_file()
    },
    "Invalid variable store path"
  );
  #[cfg(unix)]
  ensure!(
    metadata.permissions().mode() & 0o777 == if directory { 0o700 } else { 0o600 },
    "Variable store must have private permissions"
  );
  Ok(())
}

fn file(path: &Path, create: bool) -> anyhow::Result<File> {
  if !create {
    private(path, false)?;
  }
  let mut options = OpenOptions::new();
  options.read(true).write(true).create_new(create);
  #[cfg(unix)]
  options.mode(0o600);
  options
    .open(path)
    .context("Open private variable store file")
}

fn guard(root: &Path) -> anyhow::Result<File> {
  private(root, true)?;
  let path = root.join("lock");
  let lock = match file(&path, true) {
    Ok(file) => file,
    Err(error)
      if error
        .downcast_ref::<std::io::Error>()
        .is_some_and(|error| error.kind() == std::io::ErrorKind::AlreadyExists) =>
    {
      file(&path, false)?
    }
    Err(error) => return Err(error),
  };
  lock
    .try_lock_exclusive()
    .context("Another runtime owns this variable store")?;
  Ok(lock)
}

fn directory(root: &Path) -> anyhow::Result<()> {
  #[cfg(unix)]
  File::open(root)?.sync_all()?;
  let _ = root;
  Ok(())
}

pub fn create(root: &Path, template: &[u8]) -> anyhow::Result<Variables> {
  ensure!(
    !template.is_empty() && template.len() <= SIZE,
    "Invalid variable template size"
  );
  let mut builder = std::fs::DirBuilder::new();
  #[cfg(unix)]
  builder.mode(0o700);
  builder
    .create(root)
    .context("Create new private variable store")?;
  let lock = guard(root)?;
  let mut bank = vec![0xff; SIZE];
  bank[..template.len()].copy_from_slice(template);
  snapshot::write(root, &bank, 0)?;
  let log = file(&root.join("journal"), true)?;
  log.sync_all()?;
  directory(root)?;
  Ok(Variables {
    root: root.into(),
    _lock: lock,
    log,
    bank,
    poisoned: false,
    sequence: 0,
  })
}

pub fn open(root: &Path) -> anyhow::Result<Variables> {
  let lock = guard(root)?;
  let (mut bank, mut sequence) = snapshot::read(root)?;
  let saved = sequence;
  let mut log = file(&root.join("journal"), false)?;
  let retained = journal::replay(&mut log, &mut bank, &mut sequence)?;
  if retained < log.metadata()?.len() {
    log.set_len(retained)?;
    log.sync_all()?;
  }
  if retained > 0 {
    if sequence != saved {
      snapshot::write(root, &bank, sequence)?;
    }
    log.set_len(0)?;
    log.sync_all()?;
  }
  log.seek(SeekFrom::End(0))?;
  Ok(Variables {
    root: root.into(),
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
