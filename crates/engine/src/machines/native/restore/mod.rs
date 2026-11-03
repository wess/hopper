//! host restore intent; no cold-boot fallback while checkpoint handoff/recovery is pending.
mod activate;
mod fs;
mod owner;
mod prepare;
pub use prepare::{
  prepare, prepare_retained, prepare_successor, recover_preparation, Preparation, Prepared,
  Recovered, Recovery,
};
mod worker;
pub use worker::{
  launch_generation, launch_worker, pause_worker, resume_generation, resume_worker,
};
mod record;
mod successor;
use super::snapshot::Policy;
use anyhow::{ensure, Context};
use machine::snapshot::{
  manifest::{Binding, Layout, Manifest, MAX},
  package::Package,
};
pub use owner::Owner;
pub use record::{Phase, Record};
use sha2::{Digest, Sha256};
use std::{
  ffi::CString,
  fs::{File, OpenOptions},
  io::{Read, Write},
  os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
  path::Path,
};

pub struct Journal {
  root: File,
  machine: [u8; 16],
  _lock: store::lock::Lease,
}

fn name(id: [u8; 16]) -> CString {
  CString::new(id.iter().map(|b| format!("{b:02x}")).collect::<String>())
    .expect("derived restore name")
}

pub fn read(root: &File, machine: [u8; 16]) -> anyhow::Result<Option<Record>> {
  fs::private(root)?;
  let file = match fs::file(root, c"restore.hrr", 0o400, false) {
    Ok(file) => file,
    Err(error)
      if error
        .downcast_ref::<std::io::Error>()
        .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
    {
      return Ok(None)
    }
    Err(error) => return Err(error),
  };
  let mut bytes = Vec::new();
  file.take(record::SIZE as u64 + 1).read_to_end(&mut bytes)?;
  Ok(Some(Record::decode(&bytes, machine)?))
}

pub(crate) fn selection(root: &Path, id: &str) -> anyhow::Result<Option<Record>> {
  let identity = model::native::machine_identity(id).context("Invalid restore VM identity")?;
  let root = match OpenOptions::new()
    .read(true)
    .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
    .open(root)
  {
    Ok(root) => root,
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
    Err(error) => return Err(error.into()),
  };
  read(&root, identity)
}

pub(crate) fn check_start(root: &Path, id: &str) -> anyhow::Result<()> {
  let root = OpenOptions::new()
    .read(true)
    .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
    .open(root)?;
  let identity = model::native::machine_identity(id).context("Invalid restore VM identity")?;
  ensure!(
    read(&root, identity)
      .context("Read native restore selection before startup")?
      .is_none(),
    "Native VM restore is pending; preserve its generations for recovery"
  );
  Ok(())
}

impl Journal {
  /// Trusted host only; caller retains the VM operation/runtime exclusion and pinned VM root.
  pub fn open(root: &File, machine: [u8; 16]) -> anyhow::Result<Self> {
    ensure!(machine != [0; 16], "Restore VM identity is unset");
    fs::private(root)?;
    let file = match fs::file(root, c".restore", 0o600, true) {
      Ok(file) => file,
      Err(e)
        if e
          .downcast_ref::<std::io::Error>()
          .is_some_and(|e| e.kind() == std::io::ErrorKind::AlreadyExists) =>
      {
        fs::file(root, c".restore", 0o600, false)?
      }
      Err(e) => return Err(e),
    };
    let lease = store::lock::exclusive(file).context("Another host owns restore selection")?;
    read(root, machine)?;
    Ok(Self {
      root: root.try_clone()?,
      machine,
      _lock: lease,
    })
  }

  /// Commit intent only after worker admission of source/prior captures and prepared generation.
  /// Receipt checks here bind those artifacts; they do not replace full worker component admission.
  pub fn select(
    &mut self,
    record: Record,
    layout: Layout,
    policy: &Policy,
    mut check: impl FnMut() -> anyhow::Result<()>,
  ) -> anyhow::Result<()> {
    record.encode()?;
    ensure!(
      record.machine == self.machine
        && record.machine == policy.identity()
        && record.phase == Phase::Prepared
        && record.previous.is_none(),
      "Invalid initial restore selection"
    );
    policy.check()?;
    check()?;
    ensure!(
      read(&self.root, self.machine)?.is_none(),
      "Restore selection already exists; recovery is required"
    );
    self.artifacts(record, layout, &mut check)?;
    let _commit = policy.commit()?;
    policy.check()?;
    check()?;
    ensure!(
      read(&self.root, self.machine)?.is_none(),
      "Restore selection changed before commit"
    );
    self.artifacts(record, layout, &mut check)?;
    let mut file = fs::file(&self.root, c"restore.hrr", 0o600, true)?;
    file.write_all(&record.encode()?)?;
    fs::seal(&file)?;
    self.root.sync_all()?;
    policy.check()?;
    check()?;
    ensure!(
      read(&self.root, self.machine)? == Some(record),
      "Restore selection changed after commit"
    );
    Ok(())
  }

  pub fn attempted(
    &mut self,
    expected: Record,
    policy: &Policy,
    mut check: impl FnMut() -> anyhow::Result<()>,
  ) -> anyhow::Result<Record> {
    ensure!(
      expected.machine == self.machine
        && self.machine == policy.identity()
        && expected.phase == Phase::Prepared,
      "Restore attempt has invalid original selection"
    );
    policy.check()?;
    check()?;
    ensure!(
      read(&self.root, self.machine)? == Some(expected),
      "Restore attempt selection changed"
    );
    let next = Record {
      phase: Phase::Attempted,
      ..expected
    };
    let temporary = CString::new(format!(".restore{}", uuid::Uuid::new_v4().simple()))?;
    let mut file = fs::file(&self.root, &temporary, 0o600, true)?;
    file.write_all(&next.encode()?)?;
    fs::seal(&file)?;
    let _commit = policy.commit()?;
    policy.check()?;
    check()?;
    ensure!(
      read(&self.root, self.machine)? == Some(expected),
      "Restore attempt changed before commit"
    );
    ensure!(
      unsafe {
        libc::renameat(
          self.root.as_raw_fd(),
          temporary.as_ptr(),
          self.root.as_raw_fd(),
          c"restore.hrr".as_ptr(),
        )
      } == 0,
      "Commit restore attempt: {}",
      std::io::Error::last_os_error()
    );
    self.root.sync_all()?;
    policy.check()?;
    check()?;
    ensure!(
      read(&self.root, self.machine)? == Some(next),
      "Restore attempt changed after commit"
    );
    Ok(next)
  }

  fn artifacts(
    &self,
    record: Record,
    layout: Layout,
    check: &mut impl FnMut() -> anyhow::Result<()>,
  ) -> anyhow::Result<()> {
    let snapshots = fs::directory(&self.root, c"snapshots")?;
    let binding = |snapshot| Binding {
      machine: record.machine,
      snapshot,
      configuration: record.configuration,
    };
    let source = Package::open(&snapshots, binding(record.snapshot), layout, &mut *check)?;
    let _prior = Package::open(&snapshots, binding(record.rollback), layout, &mut *check)?;
    let generations = fs::directory(&self.root, c"generations")?;
    let generation = fs::directory(&generations, &name(record.generation))?;
    fs::require_absent(&generation, c"attempt.hga")?;
    let mut bytes = Vec::new();
    fs::file(&generation, c"generation.hgr", 0o400, false)?
      .take((MAX + 53) as u64)
      .read_to_end(&mut bytes)?;
    ensure!(
      (52..=MAX + 52).contains(&bytes.len())
        && &bytes[..4] == b"HGR1"
        && bytes[4..20] == record.generation,
      "Invalid selected generation receipt"
    );
    let end = bytes.len() - 32;
    ensure!(
      Sha256::digest(&bytes[..end])[..] == bytes[end..],
      "Selected generation checksum mismatch"
    );
    let manifest = Manifest::decode(&bytes[20..end], binding(record.snapshot), layout)?;
    ensure!(
      <[u8; 32]>::from(Sha256::digest(manifest.encode())) == source.manifest_fingerprint(),
      "Selected generation has another source checkpoint"
    );
    check()
  }
}
