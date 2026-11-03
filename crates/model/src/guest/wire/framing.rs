use super::{require, Direction, Message, Reply, Request, CHUNK};
use crate::GuestOs;
use serde::Serialize;
use std::io::{self, Write};

const MAGIC: &[u8; 4] = b"HPG1";

struct Payload {
  bytes: Vec<u8>,
  limit: usize,
}

impl Write for Payload {
  fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
    require(bytes.len() <= self.limit.saturating_sub(self.bytes.len()))?;
    self.bytes.extend_from_slice(bytes);
    Ok(bytes.len())
  }

  fn flush(&mut self) -> io::Result<()> {
    Ok(())
  }
}

pub(super) fn encode(message: &impl Serialize, direction: Direction) -> io::Result<Vec<u8>> {
  let mut payload = Payload {
    bytes: Vec::new(),
    limit: direction.limit(),
  };
  serde_json::to_writer(&mut payload, message)?;
  let mut frame = Vec::with_capacity(8 + payload.bytes.len());
  frame.extend_from_slice(MAGIC);
  frame.extend_from_slice(&(payload.bytes.len() as u32).to_be_bytes());
  frame.extend_from_slice(&payload.bytes);
  Ok(frame)
}

/// one message at a time; a framing error permanently closes this decoder.
pub struct Decoder {
  direction: Direction,
  guest: GuestOs,
  header: [u8; 8],
  received: usize,
  length: Option<usize>,
  payload: Vec<u8>,
  failed: bool,
}

impl Decoder {
  pub fn new(direction: Direction, guest: GuestOs) -> Self {
    Self {
      direction,
      guest,
      header: [0; 8],
      received: 0,
      length: None,
      payload: Vec::new(),
      failed: false,
    }
  }

  pub fn feed(&mut self, bytes: &[u8]) -> io::Result<bool> {
    let result = self.append(bytes);
    if result.is_err() {
      self.close();
    }
    result
  }

  fn append(&mut self, mut bytes: &[u8]) -> io::Result<bool> {
    require(!self.failed && bytes.len() <= CHUNK)?;
    if self.received < 8 {
      let count = bytes.len().min(8 - self.received);
      self.header[self.received..self.received + count].copy_from_slice(&bytes[..count]);
      self.received += count;
      bytes = &bytes[count..];
      if self.received == 8 {
        require(&self.header[..4] == MAGIC)?;
        let length = u32::from_be_bytes(self.header[4..].try_into().unwrap()) as usize;
        require(length > 0 && length <= self.direction.limit())?;
        self.length = Some(length);
        self
          .payload
          .try_reserve_exact(length)
          .map_err(io::Error::other)?;
      }
    }
    if let Some(length) = self.length {
      require(bytes.len() <= length - self.payload.len())?;
      self.payload.extend_from_slice(bytes);
      Ok(self.payload.len() == length)
    } else {
      Ok(false)
    }
  }

  pub fn take(&mut self) -> io::Result<Option<Message>> {
    require(!self.failed)?;
    if !self
      .length
      .is_some_and(|length| self.payload.len() == length)
    {
      return Ok(None);
    }
    let result = self.decode();
    if result.is_err() {
      self.close();
    } else {
      self.header = [0; 8];
      self.received = 0;
      self.length = None;
      self.payload.clear();
    }
    result.map(Some)
  }

  fn decode(&self) -> io::Result<Message> {
    match self.direction {
      Direction::Request => {
        let request: Request = serde_json::from_slice(&self.payload)?;
        require(request.valid_for(self.guest))?;
        Ok(Message::Request(request))
      }
      Direction::Reply => {
        let reply: Reply = serde_json::from_slice(&self.payload)?;
        require(reply.valid())?;
        Ok(Message::Reply(reply))
      }
    }
  }

  pub fn close(&mut self) {
    self.failed = true;
    self.payload = Vec::new();
  }
}
