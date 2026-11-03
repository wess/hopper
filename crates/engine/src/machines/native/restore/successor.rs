use super::{fs, name, read, Journal, Phase, Record};
use crate::machines::native::snapshot::Policy;
use anyhow::ensure;
use machine::snapshot::manifest::Layout;
use std::{
  ffi::CString,
  io::{Read, Write},
  os::fd::AsRawFd,
};

fn archive_name(generation: [u8; 16]) -> CString {
  CString::new(format!("restore{}.hrr", name(generation).to_string_lossy())).unwrap()
}

impl Journal {
  /// Select a freshly admitted generation while preserving the exact predecessor receipt.
  /// The caller owns stopped hardware exclusion; receipt checks do not admit checkpoint payloads.
  pub fn succeed(
    &mut self,
    expected: Record,
    next: Record,
    layout: Layout,
    policy: &Policy,
    mut check: impl FnMut() -> anyhow::Result<()>,
  ) -> anyhow::Result<()> {
    expected.encode()?;
    next.encode()?;
    ensure!(
      expected.machine == self.machine
        && next.machine == self.machine
        && policy.identity() == self.machine
        && next.configuration == expected.configuration
        && next.previous == Some(expected.generation)
        && next.generation != expected.generation
        && next.phase == Phase::Prepared,
      "Invalid restore successor binding"
    );
    policy.check()?;
    check()?;
    ensure!(
      read(&self.root, self.machine)? == Some(expected),
      "Restore predecessor changed"
    );
    // An archived generation cannot become a checkpoint candidate again.
    fs::require_absent(&self.root, &archive_name(next.generation))?;
    self.artifacts(next, layout, &mut check)?;
    let temporary = CString::new(format!(".restore{}", uuid::Uuid::new_v4().simple()))?;
    let mut file = fs::file(&self.root, &temporary, 0o600, true)?;
    file.write_all(&next.encode()?)?;
    fs::seal(&file)?;
    let _commit = policy.commit()?;
    policy.check()?;
    check()?;
    ensure!(
      read(&self.root, self.machine)? == Some(expected),
      "Restore predecessor changed before commit"
    );
    fs::require_absent(&self.root, &archive_name(next.generation))?;
    self.artifacts(next, layout, &mut check)?;
    self.archive(expected)?;
    policy.check()?;
    check()?;
    ensure!(
      read(&self.root, self.machine)? == Some(expected),
      "Restore predecessor changed after archival"
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
      "Commit restore successor: {}",
      std::io::Error::last_os_error()
    );
    self.root.sync_all()?;
    policy.check()?;
    check()?;
    ensure!(
      read(&self.root, self.machine)? == Some(next),
      "Restore successor changed after commit"
    );
    Ok(())
  }

  fn archive(&self, expected: Record) -> anyhow::Result<()> {
    let archive = archive_name(expected.generation);
    match fs::file(&self.root, &archive, 0o400, false) {
      Ok(file) => {
        let mut bytes = Vec::new();
        file
          .take(super::record::SIZE as u64 + 1)
          .read_to_end(&mut bytes)?;
        ensure!(
          Record::decode(&bytes, self.machine)? == expected,
          "Restore history conflicts with predecessor"
        );
        // Also covers a prior rename whose directory sync failed.
        self.root.sync_all()?;
        return Ok(());
      }
      Err(error)
        if error
          .downcast_ref::<std::io::Error>()
          .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) => {}
      Err(error) => return Err(error),
    }
    let temporary = CString::new(format!(".history{}", uuid::Uuid::new_v4().simple()))?;
    let mut file = fs::file(&self.root, &temporary, 0o600, true)?;
    file.write_all(&expected.encode()?)?;
    fs::seal(&file)?;
    ensure!(
      unsafe {
        libc::renameatx_np(
          self.root.as_raw_fd(),
          temporary.as_ptr(),
          self.root.as_raw_fd(),
          archive.as_ptr(),
          libc::RENAME_EXCL,
        )
      } == 0,
      "Preserve restore predecessor: {}",
      std::io::Error::last_os_error()
    );
    self.root.sync_all()?;
    Ok(())
  }
}
