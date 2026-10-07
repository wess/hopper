#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod worker;

fn main() {
  #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
  let result = worker::run();
  #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
  let result: anyhow::Result<()> = Err(anyhow::anyhow!("Native VMs require Apple silicon macOS"));
  if let Err(error) = result {
    eprintln!("Native worker stopped: {error}");
    std::process::exit(1);
  }
}
