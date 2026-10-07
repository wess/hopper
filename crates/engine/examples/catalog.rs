#[tokio::main]
async fn main() -> anyhow::Result<()> {
  let selection =
    engine::machines::windows::catalog::fetch(std::path::Path::new("/usr/bin/tar")).await?;
  println!(
    "Official ARM64 Professional catalogue selected; installer bytes: {}; SHA1: {}; catalogue SHA256: {}",
    selection.media.size, selection.media.sha1, selection.catalogue_sha256,
  );
  Ok(())
}
