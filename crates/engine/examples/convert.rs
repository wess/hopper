#[tokio::main]
async fn main() -> anyhow::Result<()> {
  let args: Vec<_> = std::env::args().skip(1).collect();
  anyhow::ensure!(
    args.len() == 2,
    "Provide a diagnostic ESD and a new output ISO path"
  );
  let tools = engine::machines::windows::assets::locate()?;
  engine::machines::windows::media::convert::build(
    std::path::Path::new(&args[0]),
    std::path::Path::new(&args[1]),
    &tools.media,
  )
  .await?;
  println!("Native media conversion completed; no guest was started");
  Ok(())
}
