use anyhow::ensure;
use engine::machines::windows::inspect;
use std::path::Path;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  let args: Vec<_> = std::env::args().skip(1).collect();
  ensure!(args.len() == 2, "Provide the installer ISO and WIM helper");
  let contents = inspect::read(
    Path::new(&args[0]),
    Path::new(&args[1]),
    Path::new("/usr/sbin/diskutil"),
  )
  .await?;
  let layout = inspect::layout(&contents, 64)?;
  println!(
    "ARM64 Professional index {}; recovery image {} bytes; recovery partition {} MiB",
    layout.image_index, layout.recovery_image_bytes, layout.recovery_mib
  );
  Ok(())
}
