use anyhow::{ensure, Context};
use serde::{de::DeserializeOwned, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub(super) async fn read<T: DeserializeOwned>(
  reader: &mut (impl AsyncRead + Unpin),
) -> anyhow::Result<T> {
  let mut prefix = [0; 8];
  reader.read_exact(&mut prefix).await?;
  ensure!(&prefix[..4] == b"HPA1", "Unsupported native agent protocol");
  let length = u32::from_le_bytes(prefix[4..].try_into()?) as usize;
  ensure!(
    (1..=4096).contains(&length),
    "Native agent header exceeds bounds"
  );
  let mut header = vec![0; length];
  reader.read_exact(&mut header).await?;
  Ok(serde_json::from_slice(&header)?)
}

pub(super) async fn write(
  writer: &mut (impl AsyncWrite + Unpin),
  header: &impl Serialize,
  payload: &[u8],
) -> anyhow::Result<()> {
  let header = serde_json::to_vec(header)?;
  ensure!(header.len() <= 4096, "Native agent header exceeds bounds");
  writer.write_all(b"HPA1").await?;
  writer
    .write_all(&(header.len() as u32).to_le_bytes())
    .await?;
  writer.write_all(&header).await?;
  writer.write_all(payload).await?;
  Ok(())
}

pub(super) fn frame_size(width: u32, height: u32) -> anyhow::Result<usize> {
  ensure!(
    (1..=4096).contains(&width) && (1..=4096).contains(&height),
    "Invalid native frame dimensions"
  );
  let size = (width as usize)
    .checked_mul(height as usize)
    .and_then(|size| size.checked_mul(4))
    .context("Native frame size overflow")?;
  ensure!(size <= 64 * 1024 * 1024, "Native frame exceeds bounds");
  Ok(size)
}
