#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn main() -> anyhow::Result<()> {
  use engine::machines::vz::mac::{acquire, download};
  use std::{sync::Arc, time::Duration};
  let runtime = tokio::runtime::Builder::new_multi_thread()
    .enable_all()
    .build()?;
  runtime.block_on(async {
    let image = acquire::latest(Arc::new(|| Ok(()))).await?;
    let client = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none())
      .timeout(Duration::from_secs(60)).build()?;
    let source = download::inspect(&client, &image.url).await?;
    let response = client.get(&source.url).header(reqwest::header::RANGE, "bytes=0-1023")
      .header(reqwest::header::IF_MATCH, &source.etag)
      .header(reqwest::header::IF_RANGE, &source.etag)
      .header(reqwest::header::ACCEPT_ENCODING, "identity").send().await?;
    anyhow::ensure!(response.status() == reqwest::StatusCode::PARTIAL_CONTENT
      && response.url().as_str() == source.url
      && response.headers().get(reqwest::header::ETAG).is_some_and(|value| value == source.etag.as_str())
      && response.headers().get(reqwest::header::CONTENT_RANGE).is_some_and(|value| value == format!("bytes 0-1023/{}", source.size).as_str())
      && response.content_length() == Some(1024), "Official restore range/identity mismatch");
    anyhow::ensure!(response.bytes().await?.len() == 1024, "Official restore range size mismatch");
    println!("Official supported macOS {}.{}.{} build {}: {} bytes, strong ETag {}; 1024-byte conditional range verified, no full IPSW download or installation", image.version[0], image.version[1], image.version[2], image.build, source.size, source.etag);
    Ok(())
  })
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("Restore download discovery requires Apple silicon macOS")
}
