use crate::guest::wire::{require, Direction, CHUNK};
use std::io;

#[derive(Default)]
pub(super) struct Frames {
  header: [u8; 8],
  received: usize,
  length: Option<usize>,
  bytes: Vec<u8>,
}

impl Frames {
  pub(super) fn empty(&self) -> bool {
    self.received == 0
  }

  pub(super) fn feed(
    &mut self,
    mut bytes: &[u8],
    direction: Direction,
  ) -> io::Result<Option<Vec<u8>>> {
    require(bytes.len() <= CHUNK)?;
    if self.received < 8 {
      let count = bytes.len().min(8 - self.received);
      self.header[self.received..self.received + count].copy_from_slice(&bytes[..count]);
      self.received += count;
      bytes = &bytes[count..];
      if self.received == 8 {
        require(&self.header[..4] == b"HPG1")?;
        let payload = u32::from_be_bytes(self.header[4..].try_into().unwrap()) as usize;
        require(payload > 0 && payload <= direction.limit())?;
        let length = payload + 40;
        self
          .bytes
          .try_reserve_exact(length)
          .map_err(io::Error::other)?;
        self.bytes.extend_from_slice(&self.header);
        self.length = Some(length);
      }
    }
    if let Some(length) = self.length {
      require(bytes.len() <= length - self.bytes.len())?;
      self.bytes.extend_from_slice(bytes);
      if self.bytes.len() == length {
        let frame = std::mem::take(&mut self.bytes);
        *self = Self::default();
        return Ok(Some(frame));
      }
    }
    Ok(None)
  }
}
