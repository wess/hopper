use super::Media;
use anyhow::{ensure, Context};
use fs2::FileExt;
use reqwest::{header, Client, StatusCode};
use std::path::{Path, PathBuf};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

async fn hash(file: &mut tokio::fs::File) -> anyhow::Result<(sha1_smol::Sha1, u64)> {
  let mut digest = sha1_smol::Sha1::new();
  let mut bytes = vec![0; 1024 * 1024];
  let mut size = 0;
  loop {
    let count = file.read(&mut bytes).await?;
    if count == 0 {
      return Ok((digest, size));
    }
    digest.update(&bytes[..count]);
    size += count as u64;
  }
}

fn range(response: &reqwest::Response, offset: u64, size: u64) -> anyhow::Result<()> {
  let value = response
    .headers()
    .get(header::CONTENT_RANGE)
    .context("Resumed installer has no Content-Range")?
    .to_str()?;
  ensure!(
    value == format!("bytes {offset}-{}/{size}", size - 1),
    "Resumed installer does not match the requested byte range"
  );
  Ok(())
}

pub async fn fetch(
  client: &Client,
  media: &Media,
  cache: &Path,
  progress: &Path,
) -> anyhow::Result<PathBuf> {
  ensure!(
    media.size > 0
      && media.size <= 12 * 1024 * 1024 * 1024
      && media.sha1.len() == 40
      && media.sha1.bytes().all(|c| c.is_ascii_hexdigit()),
    "Invalid installer download identity"
  );
  let expected = media.sha1.to_ascii_lowercase();
  let complete = cache.join(format!("{expected}.esd"));
  if complete.is_file() {
    tokio::fs::write(progress, "Verifying cached Windows installer…").await?;
    let (digest, size) = hash(&mut tokio::fs::File::open(&complete).await?).await?;
    if size == media.size && digest.digest().to_string() == expected {
      return Ok(complete);
    }
  }

  let partial = cache.join(format!("{expected}.esd.part"));
  let file = std::fs::OpenOptions::new()
    .create(true)
    .truncate(false)
    .read(true)
    .write(true)
    .open(&partial)?;
  // keep the inode lock on the file itself, including any pending async disk writes
  file
    .try_lock_exclusive()
    .context("Another VM is downloading this installer")?;
  let mut file = tokio::fs::File::from_std(file);
  if file.metadata().await?.len() > media.size {
    file.set_len(0).await?;
  }
  let (mut digest, mut written) = hash(&mut file).await?;
  if written == media.size {
    if digest.digest().to_string() == expected {
      file.sync_all().await?;
      tokio::fs::rename(&partial, &complete).await?;
      return Ok(complete);
    }
    file.set_len(0).await?;
    file.rewind().await?;
    digest = sha1_smol::Sha1::new();
    written = 0;
  }
  tokio::fs::write(
    progress,
    format!(
      "Downloading Windows installer… {}%",
      written * 100 / media.size
    ),
  )
  .await?;
  let mut request = client
    .get(&media.file_path)
    .header(header::ACCEPT_ENCODING, "identity");
  if written != 0 {
    request = request.header(header::RANGE, format!("bytes={written}-"));
  }
  let mut response = request.send().await?.error_for_status()?;
  ensure!(
    response
      .headers()
      .get(header::CONTENT_ENCODING)
      .is_none_or(|value| value == "identity"),
    "Installer download must use unencoded bytes"
  );
  let offset = match response.status() {
    StatusCode::PARTIAL_CONTENT => {
      range(&response, written, media.size)?;
      written
    }
    StatusCode::OK => 0,
    _ => anyhow::bail!("Unexpected installer download response"),
  };
  ensure!(
    response
      .content_length()
      .is_none_or(|length| length == media.size - offset),
    "Windows installer size does not match Microsoft's catalogue"
  );
  if offset == 0 {
    file.set_len(0).await?;
    file.rewind().await?;
    digest = sha1_smol::Sha1::new();
    written = 0;
  }
  let mut percent = written * 100 / media.size;
  while let Some(bytes) = response.chunk().await? {
    ensure!(
      bytes.len() as u64 <= media.size - written,
      "Installer exceeds its expected size"
    );
    file.write_all(&bytes).await?;
    digest.update(&bytes);
    written += bytes.len() as u64;
    let next = written * 100 / media.size;
    if next != percent {
      percent = next;
      tokio::fs::write(
        progress,
        format!("Downloading Windows installer… {percent}%"),
      )
      .await?;
    }
  }
  file.sync_all().await?;
  ensure!(
    written == media.size,
    "Windows installer download is incomplete; retry to resume"
  );
  if digest.digest().to_string() != expected {
    file.set_len(0).await?;
    file.sync_all().await?;
    anyhow::bail!(
      "Windows installer failed Microsoft's checksum verification; retry to download again"
    );
  }
  tokio::fs::rename(&partial, &complete).await?;
  Ok(complete)
}
