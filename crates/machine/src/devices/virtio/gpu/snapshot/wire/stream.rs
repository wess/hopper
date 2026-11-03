use anyhow::ensure;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};

pub struct Writer<'a, W, A> {
  pub output: &'a mut W,
  pub authorize: A,
  pub digest: Sha256,
}

impl<W: Write, A: FnMut() -> anyhow::Result<()>> Writer<'_, W, A> {
  pub fn data(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
    for bytes in bytes.chunks(65536) {
      (self.authorize)()?;
      self.output.write_all(bytes)?;
      self.digest.update(bytes);
      (self.authorize)()?;
    }
    Ok(())
  }
  pub fn number(&mut self, value: u64) -> anyhow::Result<()> {
    self.data(&value.to_le_bytes())
  }
  pub fn word(&mut self, value: u32) -> anyhow::Result<()> {
    self.data(&value.to_le_bytes())
  }
  pub fn byte(&mut self, value: u8) -> anyhow::Result<()> {
    self.data(&[value])
  }
}

pub struct Reader<'a, R, A> {
  pub input: &'a mut R,
  pub authorize: A,
  pub digest: Sha256,
}

impl<R: Read, A: FnMut() -> anyhow::Result<()>> Reader<'_, R, A> {
  pub fn data(&mut self, bytes: &mut [u8]) -> anyhow::Result<()> {
    for bytes in bytes.chunks_mut(65536) {
      (self.authorize)()?;
      self.input.read_exact(bytes)?;
      self.digest.update(&*bytes);
      (self.authorize)()?;
    }
    Ok(())
  }
  pub fn bytes(&mut self, count: usize) -> anyhow::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(count)?;
    bytes.resize(count, 0);
    self.data(&mut bytes)?;
    Ok(bytes)
  }
  pub fn number(&mut self) -> anyhow::Result<u64> {
    let mut bytes = [0; 8];
    self.data(&mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
  }
  pub fn word(&mut self) -> anyhow::Result<u32> {
    let mut bytes = [0; 4];
    self.data(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
  }
  pub fn boolean(&mut self) -> anyhow::Result<bool> {
    let mut bytes = [0; 1];
    self.data(&mut bytes)?;
    ensure!(bytes[0] <= 1, "GPU record boolean is invalid");
    Ok(bytes[0] == 1)
  }
}
