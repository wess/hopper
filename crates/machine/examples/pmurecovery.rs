#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "support/pmurecovery.rs"]
mod probe;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "support/cpurecords.rs"]
mod records;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn main() -> anyhow::Result<()> {
  probe::run()
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("The native hypervisor requires Apple silicon macOS")
}
