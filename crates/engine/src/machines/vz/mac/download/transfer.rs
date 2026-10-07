use super::{
  cache::{self, Cache},
  Source,
};
use crate::machines::vz::mac::Phase;
use anyhow::{ensure, Context};
use fs2::FileExt;
use machine::vz::queue::Check;
use reqwest::{header, Client, StatusCode};
use sha2::{Digest, Sha256};
use std::{path::PathBuf, time::Duration};
use tokio::{
  io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
  sync::watch::Sender,
};

async fn hash(
  file: &mut tokio::fs::File,
  size: u64,
  check: &Check,
  progress: &Sender<Phase>,
) -> anyhow::Result<(Sha256, u64)> {
  let mut digest = Sha256::new();
  let mut buffer = vec![0; 1 << 20];
  let mut total = 0;
  progress.send_replace(Phase::Verifying {
    bytes: 0,
    total: size,
  });
  loop {
    check()?;
    let count = file.read(&mut buffer).await?;
    if count == 0 {
      return Ok((digest, total));
    }
    total += count as u64;
    ensure!(total <= size, "Restore cache exceeds its expected size");
    digest.update(&buffer[..count]);
    progress.send_replace(Phase::Verifying {
      bytes: total,
      total: size,
    });
  }
}

pub(super) async fn fetch(
  client: &Client,
  source: &Source,
  mut cache: Cache,
  check: Check,
  progress: &Sender<Phase>,
) -> anyhow::Result<PathBuf> {
  match std::fs::symlink_metadata(&cache.complete) {
    Ok(_) => {
      let mut file = tokio::fs::File::from_std(cache::open_file(&cache.complete, false)?);
      let (digest, size) = hash(&mut file, source.size, &check, progress).await?;
      ensure!(
        size == source.size
          && cache
            .receipt
            .sha256
            .as_ref()
            .is_some_and(|expected| *expected == format!("{:x}", digest.finalize())),
        "Cached macOS restore image failed verification; preserve it for recovery"
      );
      check()?;
      return Ok(cache.complete);
    }
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
    Err(error) => return Err(error.into()),
  }
  let file = cache::open_file(&cache.partial, true)?;
  file
    .try_lock_exclusive()
    .context("Restore writes from another acquisition are still active")?;
  // the partial inode lock also follows outstanding async writes on cancellation.
  let mut file = tokio::fs::File::from_std(file);
  let (mut digest, mut written) = hash(&mut file, source.size, &check, progress).await?;
  if written != source.size {
    ensure!(
      fs2::available_space(&cache.directory)? >= source.size - written + (256 << 20),
      "macOS restore download needs more free disk space; partial media is preserved"
    );
    let mut request = client
      .get(&source.url)
      .header(header::ACCEPT_ENCODING, "identity")
      .header(header::IF_MATCH, &source.etag)
      .timeout(Duration::from_secs(6 * 60 * 60));
    if written != 0 {
      request = request
        .header(header::RANGE, format!("bytes={written}-"))
        .header(header::IF_RANGE, &source.etag);
    }
    progress.send_replace(Phase::Downloading {
      bytes: written,
      total: source.size,
    });
    let mut response = super::checked(&check, request.send())
      .await??
      .error_for_status()?;
    check()?;
    ensure!(
      response.url().as_str() == source.url
        && response
          .headers()
          .get(header::ETAG)
          .is_some_and(|etag| etag == source.etag.as_str())
        && response
          .headers()
          .get(header::CONTENT_ENCODING)
          .is_none_or(|value| value == "identity"),
      "Restore download changed origin, encoding or identity"
    );
    let offset = match response.status() {
      StatusCode::PARTIAL_CONTENT => {
        ensure!(
          response
            .headers()
            .get(header::CONTENT_RANGE)
            .context("Resumed restore image has no range")?
            .to_str()?
            == format!("bytes {written}-{}/{}", source.size - 1, source.size),
          "Restore download range mismatch"
        );
        written
      }
      StatusCode::OK => 0,
      _ => anyhow::bail!("Unexpected restore download response"),
    };
    ensure!(
      response
        .content_length()
        .is_none_or(|length| length == source.size - offset),
      "Restore download length mismatch"
    );
    if offset == 0 {
      ensure!(
        fs2::available_space(&cache.directory)? + written >= source.size + (256 << 20),
        "Restarting restore download needs more free disk space"
      );
      file.set_len(0).await?;
      file.rewind().await?;
      written = 0;
      digest = Sha256::new();
    }
    let streamed: anyhow::Result<()> = async {
      while let Some(bytes) = super::checked(&check, response.chunk()).await?? {
        check()?;
        ensure!(
          bytes.len() as u64 <= source.size - written,
          "Restore download exceeds its expected size"
        );
        file.write_all(&bytes).await?;
        digest.update(&bytes);
        written += bytes.len() as u64;
        progress.send_replace(Phase::Downloading {
          bytes: written,
          total: source.size,
        });
      }
      Ok(())
    }
    .await;
    if let Err(error) = streamed {
      // finish accepted writes before returning a retryable partial image.
      file
        .sync_all()
        .await
        .context("Flush interrupted restore download")?;
      return Err(error);
    }
  }
  file.sync_all().await?;
  check()?;
  ensure!(
    written == source.size,
    "Restore download interrupted; retry to resume"
  );
  let digest = format!("{:x}", digest.finalize());
  if let Some(expected) = &cache.receipt.sha256 {
    ensure!(
      *expected == digest,
      "Restore partial image failed verification; preserve it for recovery"
    );
  }
  cache.receipt.sha256 = Some(digest);
  cache.save()?;
  check()?;
  cache.publish()?;
  Ok(cache.complete)
}
