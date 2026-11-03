#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn main() -> anyhow::Result<()> {
  use machine::hypervisor::{self as hv, gic::state};

  let saved = {
    let vm = hv::create()?;
    let gic = hv::gic::create_msi(&vm, 0x08000000, 0x0a000000, 0x30000000, 64, 32)?;
    let _cpu = hv::cpu(&vm)?;
    hv::gic::signal(&gic, 33, true)?;
    hv::gic::message(&gic, 0x30000040, 64)?;
    hv::gic::message(&gic, 0x30000040, 95)?;
    let saved = state::capture(&gic)?;
    anyhow::ensure!(hv::gic::read(&gic, 0x204)? & 2 != 0);
    anyhow::ensure!(hv::gic::read(&gic, 0x208)? & 0x80000001 == 0x80000001);
    saved
  };
  let vm = hv::create()?;
  let gic = hv::gic::create_msi(&vm, 0x08000000, 0x0a000000, 0x30000000, 64, 32)?;
  let _cpu = hv::cpu(&vm)?;
  anyhow::ensure!(hv::gic::read(&gic, 0x204)? & 2 == 0);
  anyhow::ensure!(hv::gic::read(&gic, 0x208)? & 0x80000001 == 0);
  let decoded = state::State::from_bytes(saved.bytes())?;
  state::restore(&gic, &decoded)?;
  anyhow::ensure!(hv::gic::read(&gic, 0x204)? & 2 != 0);
  anyhow::ensure!(hv::gic::read(&gic, 0x208)? & 0x80000001 == 0x80000001);
  anyhow::ensure!(state::State::from_bytes(&[]).is_err());
  anyhow::ensure!(state::State::from_bytes(&vec![0; 16 * 1024 * 1024 + 1]).is_err());
  // incompatible serialized state must fail through the SDK, without replacing it ourselves.
  let invalid = state::State::from_bytes(&[0; 64])?;
  anyhow::ensure!(state::restore(&gic, &invalid).is_err());
  anyhow::ensure!(hv::gic::read(&gic, 0x204)? & 2 != 0);
  anyhow::ensure!(hv::gic::read(&gic, 0x208)? & 0x80000001 == 0x80000001);
  println!(
    "Interrupt state: {} bytes; fresh-VM wired/MSI restore and invalid-state rejection passed",
    saved.bytes().len()
  );
  Ok(())
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("The native hypervisor requires Apple silicon macOS")
}
