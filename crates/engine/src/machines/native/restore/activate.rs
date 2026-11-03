use super::{fs, read, Journal, Phase, Record};
use crate::machines::native::snapshot::{Commit, Policy};
use anyhow::ensure;
use std::{ffi::CString, io::Write, os::fd::AsRawFd};

impl Journal {
  // only the held worker's matching admission permits activation; retain the guard through resume.
  pub(super) fn activate(
    &self,
    expected: Record,
    policy: &Policy,
    mut check: impl FnMut() -> anyhow::Result<()>,
  ) -> anyhow::Result<Commit> {
    ensure!(
      expected.phase == Phase::Attempted
        && expected.machine == self.machine
        && self.machine == policy.identity(),
      "Invalid restored activation selection"
    );
    policy.check()?;
    check()?;
    ensure!(
      read(&self.root, self.machine)? == Some(expected),
      "Restore activation selection changed"
    );
    let active = Record {
      phase: Phase::Active,
      ..expected
    };
    let name = CString::new(format!(".restore{}", uuid::Uuid::new_v4().simple()))?;
    let mut file = fs::file(&self.root, &name, 0o600, true)?;
    file.write_all(&active.encode()?)?;
    fs::seal(&file)?;
    let commit = policy.commit()?;
    policy.check()?;
    check()?;
    ensure!(
      read(&self.root, self.machine)? == Some(expected),
      "Restore activation changed before commit"
    );
    ensure!(
      unsafe {
        libc::renameat(
          self.root.as_raw_fd(),
          name.as_ptr(),
          self.root.as_raw_fd(),
          c"restore.hrr".as_ptr(),
        )
      } == 0,
      "Commit restored activation: {}",
      std::io::Error::last_os_error()
    );
    self.root.sync_all()?;
    policy.check()?;
    check()?;
    ensure!(
      read(&self.root, self.machine)? == Some(active),
      "Restore activation changed after commit"
    );
    Ok(commit)
  }
}
