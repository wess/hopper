use super::files;
use anyhow::{ensure, Context};
use fs2::FileExt;
use machine::vz::queue::Check;
use reqwest::{header, Client, StatusCode};
use sha2::{Digest, Sha256};
use std::{
  path::{Path, PathBuf},
  time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

async fn hash(
  file: &mut tokio::fs::File,
  size: u64,
  check: &Check,
) -> anyhow::Result<(Sha256, u64)> {
  let mut digest = Sha256::new();
  let mut buffer = vec![0; 1024 * 1024];
  let mut total = 0;
  loop {
    check()?;
    let count = file.read(&mut buffer).await?;
    if count == 0 {
      return Ok((digest, total));
    }
    total += count as u64;
    ensure!(total <= size, "Linux installer exceeds its pinned size");
    digest.update(&buffer[..count]);
  }
}

pub async fn fetch(
  client: &Client,
  url: &str,
  size: u64,
  checksum: &str,
  cache: &Path,
  check: Check,
) -> anyhow::Result<PathBuf> {
  ensure!(
    size > 0
      && size <= 16 << 30
      && checksum.len() == 64
      && checksum.bytes().all(|c| c.is_ascii_hexdigit()),
    "Invalid Linux media identity"
  );
  check()?;
  ensure!(cache.is_absolute(), "Linux media cache must be absolute");
  files::directory(cache)?;
  let checksum = checksum.to_ascii_lowercase();
  let complete = cache.join(format!("{checksum}.iso"));
  match std::fs::symlink_metadata(&complete) {
    Ok(_) => {
      let mut file = tokio::fs::File::from_std(files::open(&complete, false)?);
      let (digest, total) = hash(&mut file, size, &check).await?;
      ensure!(
        total == size && format!("{:x}", digest.finalize()) == checksum,
        "Cached Linux media failed verification; preserve it for recovery"
      );
      check()?;
      return Ok(complete);
    }
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
    Err(error) => return Err(error.into()),
  }
  let partial = cache.join(format!("{checksum}.part"));
  let file = files::open(&partial, true)?;
  file
    .try_lock_exclusive()
    .context("Another VM is downloading Linux media")?;
  // the inode lock follows pending async writes after caller cancellation.
  let mut file = tokio::fs::File::from_std(file);
  let (mut digest, mut written) = hash(&mut file, size, &check).await?;
  if written == size {
    check()?;
    if format!("{:x}", digest.clone().finalize()) != checksum {
      file.set_len(0).await?;
      file.sync_all().await?;
      anyhow::bail!("Partial Linux media failed verification; retry to download again");
    }
    file.sync_all().await?;
    check()?;
    files::publish(&partial, &complete)?;
    return Ok(complete);
  }
  ensure!(
    fs2::available_space(cache)? >= size - written + (64 << 20),
    "Linux installer download needs more free disk space"
  );
  let mut request = client
    .get(url)
    .header(header::ACCEPT_ENCODING, "identity")
    .timeout(Duration::from_secs(6 * 60 * 60));
  if written != 0 {
    request = request.header(header::RANGE, format!("bytes={written}-"));
  }
  let mut response = request.send().await?.error_for_status()?;
  check()?;
  ensure!(
    response.url().as_str() == url,
    "Linux media redirects are not permitted"
  );
  ensure!(
    response
      .headers()
      .get(header::CONTENT_ENCODING)
      .is_none_or(|value| value == "identity"),
    "Linux media must use unencoded bytes"
  );
  let offset = match response.status() {
    StatusCode::PARTIAL_CONTENT => {
      ensure!(
        response
          .headers()
          .get(header::CONTENT_RANGE)
          .context("Resumed Linux media has no range")?
          .to_str()?
          == format!("bytes {written}-{}/{size}", size - 1),
        "Linux media range mismatch"
      );
      written
    }
    StatusCode::OK => 0,
    _ => anyhow::bail!("Unexpected Linux media response"),
  };
  ensure!(
    response
      .content_length()
      .is_none_or(|length| length == size - offset),
    "Linux media length mismatch"
  );
  if offset == 0 {
    ensure!(
      fs2::available_space(cache)? + written >= size + (64 << 20),
      "Linux installer download needs more free disk space"
    );
    file.set_len(0).await?;
    file.rewind().await?;
    digest = Sha256::new();
    written = 0;
  }
  while let Some(bytes) = response.chunk().await? {
    check()?;
    ensure!(
      bytes.len() as u64 <= size - written,
      "Linux media exceeds its pinned size"
    );
    file.write_all(&bytes).await?;
    digest.update(&bytes);
    written += bytes.len() as u64;
  }
  file.sync_all().await?;
  check()?;
  ensure!(
    written == size,
    "Linux download interrupted; retry to resume"
  );
  if format!("{:x}", digest.finalize()) != checksum {
    file.set_len(0).await?;
    file.sync_all().await?;
    anyhow::bail!("Linux media checksum mismatch; retry to download again");
  }
  check()?;
  files::publish(&partial, &complete)?;
  Ok(complete)
}
