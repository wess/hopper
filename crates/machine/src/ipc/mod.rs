//! Private parent/worker pipes. Each packet is HPV1, a little-endian u32 JSON
//! length, and bounded JSON. Frame replies append width * height * 4 RGBA bytes.
//! Request zero is reserved for unsolicited guest shutdown/reset. Clients consume
//! one complete reply before sending another request. Start accepts host paths
//! only from the parent; this channel is not exposed as a guest or agent API.

mod stream;
pub use stream::{receive, send, Stream};

use anyhow::ensure;
use model::native::{Response, Result};
use serde::{de::DeserializeOwned, Serialize};
use std::io::{Read, Write};

pub const HEADER_LIMIT: usize = 16 * 1024;
pub const FRAME_LIMIT: usize = 64 * 1024 * 1024;
const MAGIC: [u8; 4] = *b"HPV1";

pub fn frame_size(width: u32, height: u32) -> anyhow::Result<usize> {
  ensure!(
    width > 0 && height > 0 && width <= 4096 && height <= 4096,
    "Invalid native display dimensions"
  );
  let size = width as usize * height as usize * 4;
  ensure!(size <= FRAME_LIMIT, "Native display exceeds capture limit");
  Ok(size)
}

pub fn payload_size(response: &Response) -> anyhow::Result<usize> {
  match response.result {
    Result::Frame { width, height, .. } => frame_size(width, height),
    _ => Ok(0),
  }
}

pub fn encode(header: &impl Serialize, payload: &[u8]) -> anyhow::Result<Vec<u8>> {
  ensure!(payload.len() <= FRAME_LIMIT, "Native payload exceeds limit");
  let json = serde_json::to_vec(header)?;
  ensure!(
    !json.is_empty() && json.len() <= HEADER_LIMIT,
    "Native header exceeds limit"
  );
  let mut bytes = Vec::with_capacity(8 + json.len() + payload.len());
  bytes.extend(MAGIC);
  bytes.extend((json.len() as u32).to_le_bytes());
  bytes.extend(json);
  bytes.extend(payload);
  Ok(bytes)
}

pub fn header_length(prefix: &[u8; 8]) -> anyhow::Result<usize> {
  ensure!(prefix[..4] == MAGIC, "Unknown native protocol version");
  let length = u32::from_le_bytes(prefix[4..].try_into()?) as usize;
  ensure!(
    length > 0 && length <= HEADER_LIMIT,
    "Native header exceeds limit"
  );
  Ok(length)
}

pub fn read_header<T: DeserializeOwned>(reader: &mut impl Read) -> anyhow::Result<T> {
  let mut prefix = [0; 8];
  reader.read_exact(&mut prefix)?;
  let mut json = vec![0; header_length(&prefix)?];
  reader.read_exact(&mut json)?;
  Ok(serde_json::from_slice(&json)?)
}

pub fn write_request(
  writer: &mut impl Write,
  request: &model::native::Request,
) -> anyhow::Result<()> {
  writer.write_all(&encode(request, &[])?)?;
  writer.flush()?;
  Ok(())
}

pub fn read_response(reader: &mut impl Read) -> anyhow::Result<(Response, Vec<u8>)> {
  let response: Response = read_header(reader)?;
  let mut payload = vec![0; payload_size(&response)?];
  reader.read_exact(&mut payload)?;
  Ok((response, payload))
}
