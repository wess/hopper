use super::Entry;
use anyhow::ensure;
use std::io::{self, Read};

pub struct Checked<'a, R, A> {
  entry: &'a Entry,
  input: R,
  authorize: A,
  count: u64,
  tail: [u8; 32],
  failed: bool,
}

impl<'a, R: Read, A: FnMut() -> anyhow::Result<()>> Checked<'a, R, A> {
  pub(super) fn new(entry: &'a Entry, input: R, authorize: A) -> Self {
    Self {
      entry,
      input,
      authorize,
      count: 0,
      tail: [0; 32],
      failed: false,
    }
  }

  /// Every component decoder must succeed before this admits its consumed footer.
  pub fn finish(mut self) -> anyhow::Result<()> {
    (self.authorize)()?;
    ensure!(!self.failed, "Snapshot component reader failed");
    let mut extra = [0];
    ensure!(
      self.read(&mut extra)? == 0,
      "Snapshot component has trailing bytes"
    );
    ensure!(
      self.count == self.entry.length && self.tail == self.entry.checksum,
      "Consumed snapshot component does not match its manifest"
    );
    (self.authorize)()
  }

  fn next(&mut self, output: &mut [u8]) -> anyhow::Result<usize> {
    ensure!(!self.failed, "Snapshot component reader failed");
    (self.authorize)()?;
    let count = self.input.read(output)?;
    if let Err(error) = (self.authorize)() {
      output[..count].fill(0);
      return Err(error);
    }
    let total = self
      .count
      .checked_add(count as u64)
      .ok_or_else(|| anyhow::anyhow!("Snapshot component length overflow"))?;
    ensure!(
      total <= self.entry.length,
      "Snapshot component exceeds its manifest length"
    );
    self.count = total;
    if count >= 32 {
      self.tail.copy_from_slice(&output[count - 32..count]);
    } else if count != 0 {
      self.tail.copy_within(count..32, 0);
      self.tail[32 - count..].copy_from_slice(&output[..count]);
    }
    Ok(count)
  }
}

impl<R: Read, A: FnMut() -> anyhow::Result<()>> Read for Checked<'_, R, A> {
  fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
    let count = output.len().min(65536);
    match self.next(&mut output[..count]) {
      Ok(count) => Ok(count),
      Err(error) => {
        self.failed = true;
        Err(io::Error::other(error))
      }
    }
  }
}
