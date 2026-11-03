use super::fs;
use crate::snapshot::manifest::{Entry, Record};
use anyhow::ensure;
use sha2::{Digest, Sha256};
use std::{
  fs::File,
  io::{self, Seek, SeekFrom, Write},
};
use zeroize::Zeroizing;

pub(super) struct Writer<'a, A> {
  file: File,
  authorize: &'a mut A,
  limit: u64,
  count: u64,
  tail: Zeroizing<Vec<u8>>,
  digest: Sha256,
  failed: bool,
}

impl<'a, A: FnMut() -> anyhow::Result<()>> Writer<'a, A> {
  pub(super) fn new(file: File, authorize: &'a mut A, limit: u64) -> Self {
    Self {
      file,
      authorize,
      limit,
      count: 0,
      tail: Zeroizing::new(Vec::new()),
      digest: Sha256::new(),
      failed: false,
    }
  }

  pub(super) fn finish(self, record: Record) -> anyhow::Result<Entry> {
    (self.authorize)()?;
    ensure!(
      !self.failed && self.count >= 36 && self.tail.len() == 32,
      "Snapshot component write failed or is incomplete"
    );
    let checksum: [u8; 32] = self.digest.finalize().into();
    ensure!(
      checksum[..] == self.tail[..],
      "Snapshot component checksum is invalid"
    );
    self.file.set_len(self.count)?;
    fs::seal(&self.file)?;
    (self.authorize)()?;
    Ok(Entry {
      record,
      length: self.count,
      checksum,
    })
  }

  fn next(&mut self, bytes: &[u8]) -> anyhow::Result<usize> {
    ensure!(!self.failed, "Snapshot component writer failed");
    (self.authorize)()?;
    let count = bytes.len().min(65536);
    ensure!(
      count as u64 <= self.limit - self.count,
      "Snapshot component exceeds its bound"
    );
    let count = if bytes[..count].iter().all(|byte| *byte == 0) {
      self.file.seek(SeekFrom::Current(count as i64))?;
      count
    } else {
      self.file.write(&bytes[..count])?
    };
    self.count += count as u64;
    self.tail.extend_from_slice(&bytes[..count]);
    if self.tail.len() > 32 {
      let prefix = self.tail.len() - 32;
      self.digest.update(&self.tail[..prefix]);
      self.tail.copy_within(prefix.., 0);
      self.tail.truncate(32);
    }
    (self.authorize)()?;
    Ok(count)
  }
}

impl<A: FnMut() -> anyhow::Result<()>> Write for Writer<'_, A> {
  fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
    match self.next(bytes) {
      Ok(count) => Ok(count),
      Err(error) => {
        self.failed = true;
        Err(io::Error::other(error))
      }
    }
  }

  fn flush(&mut self) -> io::Result<()> {
    if self.failed {
      return Err(io::Error::other("Snapshot component writer failed"));
    }
    Ok(())
  }
}
