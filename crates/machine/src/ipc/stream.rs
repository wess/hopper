use super::{encode, header_length, payload_size};
use anyhow::ensure;
use model::native::{Request, Response};
use std::io::{ErrorKind, Read, Write};

#[derive(Default)]
pub struct Stream {
  input: Vec<u8>,
  output: Vec<u8>,
  written: usize,
}

/// read one bounded command without waiting on a pipe; EOF belongs to the parent.
pub fn receive(stream: &mut Stream, reader: &mut impl Read) -> anyhow::Result<Option<Request>> {
  loop {
    let total = if stream.input.len() < 8 {
      8
    } else {
      8 + header_length(stream.input[..8].try_into()?)?
    };
    if stream.input.len() == total && total > 8 {
      let request: Request = serde_json::from_slice(&stream.input[8..])?;
      ensure!(request.id != 0, "Native request identity is reserved");
      stream.input.clear();
      return Ok(Some(request));
    }
    let mut bytes = [0; 4096];
    let length = bytes.len().min(total - stream.input.len());
    match reader.read(&mut bytes[..length]) {
      Ok(0) => anyhow::bail!("Native parent disconnected"),
      Ok(length) => stream.input.extend_from_slice(&bytes[..length]),
      Err(error) if error.kind() == ErrorKind::WouldBlock => return Ok(None),
      Err(error) if error.kind() == ErrorKind::Interrupted => continue,
      Err(error) => return Err(error.into()),
    }
  }
}

/// retain at most one reply; a slow parent never blocks a guest CPU owner.
pub fn send(stream: &mut Stream, response: &Response, payload: &[u8]) -> anyhow::Result<()> {
  ensure!(!stream.pending(), "Native response is still pending");
  ensure!(
    payload_size(response)? == payload.len(),
    "Native payload length does not match its header"
  );
  stream.output = encode(response, payload)?;
  stream.written = 0;
  Ok(())
}

impl Stream {
  pub fn pending(&self) -> bool {
    self.written < self.output.len()
  }

  pub fn flush(&mut self, writer: &mut impl Write) -> anyhow::Result<usize> {
    let before = self.written;
    let limit = (self.written + 64 * 1024).min(self.output.len());
    while self.written < limit {
      match writer.write(&self.output[self.written..limit]) {
        Ok(0) => anyhow::bail!("Native parent stopped receiving"),
        Ok(length) => self.written += length,
        Err(error) if error.kind() == ErrorKind::WouldBlock => break,
        Err(error) if error.kind() == ErrorKind::Interrupted => continue,
        Err(error) => return Err(error.into()),
      }
    }
    let progress = self.written - before;
    if !self.pending() {
      self.output.clear();
      self.written = 0;
    }
    Ok(progress)
  }
}
