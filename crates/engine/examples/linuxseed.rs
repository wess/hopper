#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn main() -> anyhow::Result<()> {
  use anyhow::ensure;
  use engine::machines::linux::{provision, seed};
  use std::{io::Write, os::unix::fs::OpenOptionsExt, path::PathBuf};

  let args: Vec<_> = std::env::args_os().skip(1).collect();
  ensure!(args.len() == 1, "Provide a new diagnostic seed path");
  let path = PathBuf::from(&args[0]);
  ensure!(path.is_absolute(), "Diagnostic path must be absolute");
  let id = "8197e0f0-0603-43e9-a817-eaf7ab0327af";
  let plan = provision::prepare(id, &provision::accounts())?;
  let mut file = std::fs::OpenOptions::new()
    .create_new(true)
    .write(true)
    .mode(0o600)
    .open(path)?;
  file.write_all(&seed::image(&plan)?)?;
  file.sync_all()?;
  println!("Created disposable NoCloud seed with generated guest account hashes; no credentials were printed or stored in the host keychain");
  Ok(())
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("Native seed probe requires Apple silicon macOS")
}
