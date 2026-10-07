mod cache;
mod transfer;

use super::Phase;
use anyhow::{ensure, Context};
use machine::vz::queue::Check;
use reqwest::{header, Client, StatusCode, Url};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::sync::watch::Sender;

pub(crate) async fn checked<T>(
  check: &Check,
  future: impl std::future::Future<Output = T>,
) -> anyhow::Result<T> {
  tokio::pin!(future);
  loop {
    check()?;
    tokio::select! {
      result = &mut future => { check()?; return Ok(result); },
      _ = tokio::time::sleep(std::time::Duration::from_millis(25)) => {},
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Source {
  pub url: String,
  pub size: u64,
  pub etag: String,
}

pub fn official(url: &str) -> anyhow::Result<()> {
  let url = Url::parse(url)?;
  ensure!(
    url.scheme() == "https"
      && url.username().is_empty()
      && url.password().is_none()
      && url.port().is_none()
      && url.fragment().is_none()
      && matches!(
        url.host_str(),
        Some("updates.cdn-apple.com" | "updates-http.cdn-apple.com")
      ),
    "Restore URL must use an official Apple HTTPS origin"
  );
  Ok(())
}

pub async fn inspect(client: &Client, url: &str) -> anyhow::Result<Source> {
  official(url)?;
  let response = client
    .head(url)
    .header(header::ACCEPT_ENCODING, "identity")
    .send()
    .await?;
  ensure!(
    response.status() == StatusCode::OK && response.url().as_str() == url,
    "Unexpected macOS restore metadata response"
  );
  let size = response
    .headers()
    .get(header::CONTENT_LENGTH)
    .context("Restore metadata has no size")?
    .to_str()?
    .parse()?;
  let etag = response
    .headers()
    .get(header::ETAG)
    .context("Restore metadata has no strong ETag")?
    .to_str()?
    .to_owned();
  let source = Source {
    url: url.into(),
    size,
    etag,
  };
  validate(&source)?;
  ensure!(
    response
      .headers()
      .get(header::CONTENT_ENCODING)
      .is_none_or(|value| value == "identity"),
    "Restore metadata must describe unencoded bytes"
  );
  Ok(source)
}

fn validate(source: &Source) -> anyhow::Result<()> {
  ensure!(
    (1..=64 << 30).contains(&source.size)
      && source.url.len() <= 4096
      && (2..=512).contains(&source.etag.len())
      && source.etag.starts_with('"')
      && source.etag.ends_with('"')
      && !source.etag.bytes().any(|byte| byte < 0x20 || byte == 0x7f),
    "Restore download identity is invalid"
  );
  Ok(())
}

pub async fn fetch(
  client: &Client,
  source: &Source,
  root: &Path,
  check: Check,
  progress: &Sender<Phase>,
) -> anyhow::Result<PathBuf> {
  validate(source)?;
  check()?;
  let cache = cache::open(root, source)?;
  transfer::fetch(client, source, cache, check, progress).await
}
