use super::{Variables, SIZE};
use crate::{devices::flash, hypervisor::ram::Capture};
use anyhow::ensure;
use std::{fs::File, path::Path};

/// Trusted fresh restore namespace only; caller retains package/policy/operation ownership.
/// Never replaces a store. A failure after creation preserves the new namespace for recovery.
pub fn from_snapshot(
  root: &Path,
  memory: &Capture,
  command: &flash::snapshot::State,
  mut authorize: impl FnMut() -> anyhow::Result<()>,
) -> anyhow::Result<Variables> {
  authorize()?;
  ensure!(
    root.is_absolute(),
    "Restore variable namespace must be absolute"
  );
  prepare(memory, command, &mut authorize, |bytes| {
    super::create(root, bytes)
  })
}

/// Restore only into the fixed, exclusive variables child of a pinned private stage.
/// Caller retains package, original authorization and operation ownership throughout.
pub fn from_snapshot_at(
  parent: &File,
  memory: &Capture,
  command: &flash::snapshot::State,
  mut authorize: impl FnMut() -> anyhow::Result<()>,
) -> anyhow::Result<Variables> {
  authorize()?;
  prepare(memory, command, &mut authorize, |bytes| {
    super::create_at(parent, bytes)
  })
}

fn prepare(
  memory: &Capture,
  command: &flash::snapshot::State,
  authorize: &mut impl FnMut() -> anyhow::Result<()>,
  create: impl FnOnce(&[u8]) -> anyhow::Result<Variables>,
) -> anyhow::Result<Variables> {
  geometry(memory)?;
  let mut bank = zeroize::Zeroizing::new(vec![0; SIZE]);
  for (index, bytes) in bank.chunks_mut(65536).enumerate() {
    authorize()?;
    memory.read(index * 65536, bytes)?;
  }
  // command state is validated but pending writes must not enter the durable bank early.
  let flash = flash::snapshot::restore(command, bank.to_vec())?;
  authorize()?;
  let store = create(flash::bytes(&flash))?;
  authorize()?;
  matches(&store, memory)?;
  authorize()?;
  Ok(store)
}

pub(crate) fn matches(store: &Variables, memory: &Capture) -> anyhow::Result<()> {
  geometry(memory)?;
  ensure!(!store.poisoned, "Restored variable store requires recovery");
  let mut chunk = zeroize::Zeroizing::new([0; 65536]);
  for (index, expected) in store.bank.chunks(65536).enumerate() {
    memory.read(index * 65536, &mut chunk[..expected.len()])?;
    ensure!(
      &chunk[..expected.len()] == expected,
      "Persistent variables do not match captured NVRAM"
    );
  }
  Ok(())
}

fn geometry(memory: &Capture) -> anyhow::Result<()> {
  ensure!(
    memory.guest() == 0x4000000 && memory.size() == SIZE,
    "Restored variable bank geometry is invalid"
  );
  Ok(())
}
