//! Official installer selection; catalogue origin and body bounds precede CAB extraction.

use super::setup::process;
use crate::machines::media::{catalogue, Media};
use anyhow::{ensure, Context};
use reqwest::{redirect, Client, Response, Url};
use sha2::{Digest, Sha256};
use std::{path::Path, time::Duration};

pub const ENDPOINT: &str = "https://go.microsoft.com/fwlink?linkid=2156292";
const LIMIT: usize = 4 * 1024 * 1024;

pub struct Selection {
  pub media: Media,
  pub catalogue_sha256: String,
  pub catalogue: Vec<u8>,
}

pub async fn fetch(archive: &Path) -> anyhow::Result<Selection> {
  let client = Client::builder()
    .https_only(true)
    .connect_timeout(Duration::from_secs(30))
    .read_timeout(Duration::from_secs(30))
    .redirect(redirect::Policy::custom(|attempt| {
      if attempt.previous().len() >= 5 || !origin(attempt.url()) {
        attempt.error("Untrusted Windows catalogue redirect")
      } else {
        attempt.follow()
      }
    }))
    .build()?;
  let response = client
    .get(ENDPOINT)
    .timeout(Duration::from_secs(60))
    .send()
    .await?
    .error_for_status()?;
  ensure!(origin(response.url()), "Untrusted Windows catalogue origin");
  decode(&body(response).await?, archive).await
}

pub fn origin(url: &Url) -> bool {
  url.scheme() == "https"
    && matches!(
      url.host_str(),
      Some("go.microsoft.com" | "download.microsoft.com")
    )
    && url.username().is_empty()
    && url.password().is_none()
    && url.port().is_none()
}

/// Bound the response before allocating or passing its bytes to an archive helper.
pub async fn body(mut response: Response) -> anyhow::Result<Vec<u8>> {
  ensure!(
    response.status().is_success(),
    "Windows catalogue request failed"
  );
  ensure!(
    response
      .content_length()
      .is_none_or(|size| size <= LIMIT as u64),
    "Windows catalogue exceeds its bound"
  );
  let mut bytes = Vec::new();
  while let Some(chunk) = response.chunk().await? {
    ensure!(
      chunk.len() <= LIMIT - bytes.len(),
      "Windows catalogue exceeds its bound"
    );
    bytes.extend_from_slice(&chunk);
  }
  ensure!(!bytes.is_empty(), "Empty Windows catalogue");
  Ok(bytes)
}

/// Decode supplied catalogue bytes. Only `fetch` establishes their official HTTPS origin.
pub async fn decode(bytes: &[u8], archive: &Path) -> anyhow::Result<Selection> {
  ensure!(
    (1..=LIMIT).contains(&bytes.len()),
    "Invalid Windows catalogue size"
  );
  let root = tempfile::tempdir()?;
  let cab = root.path().join("catalogue.cab");
  std::fs::write(&cab, bytes)?;
  let xml = process::run(
    archive,
    &[
      "-xOf".into(),
      cab.to_str().context("Invalid catalogue path")?.into(),
      "products.xml".into(),
    ],
    None,
    None,
    16 * 1024 * 1024,
  )
  .await?;
  // preserve Microsoft's advertised transport; the delivery host may not support HTTPS.
  let media = catalogue(std::str::from_utf8(&xml)?)?;
  Ok(Selection {
    media,
    catalogue_sha256: format!("{:x}", Sha256::digest(bytes)),
    catalogue: bytes.to_vec(),
  })
}
