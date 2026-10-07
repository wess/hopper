#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn main() -> anyhow::Result<()> {
  use anyhow::ensure;
  use engine::machines::linux::{boot, iso, kernel};
  use machine::vz::queue::Check;
  use std::{path::PathBuf, sync::Arc};
  let args: Vec<_> = std::env::args_os().skip(1).collect();
  ensure!(
    args.len() == 1,
    "Provide diagnostic installer media; this probe never boots it"
  );
  let media = PathBuf::from(&args[0]);
  let check: Check = Arc::new(|| Ok(()));
  let mut source = std::fs::File::open(&media)?;
  let bytes = iso::read(&mut source, "casper/vmlinuz", kernel::MAXIMUM, &check)?;
  let decoded = kernel::decode(&bytes, &check)?;
  let root = tempfile::tempdir()?;
  let staged = boot::stage(&media, root.path(), &check)?;
  let mut staged = std::fs::File::open(staged.path().join("installer"))?;
  let configuration = iso::read(&mut staged, "boot/grub/grub.cfg", 64 << 10, &check)?;
  ensure!(
    configuration.starts_with(boot::CONFIGURATION.as_bytes()),
    "Unattended boot flags are missing"
  );
  println!("ARM64 installer kernel validated ({} bytes decoded); private EFI GRUB staging verified. No full-media integrity check, VM boot or OS installation was performed", decoded.len());
  Ok(())
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("Native boot preparation requires Apple silicon macOS")
}
