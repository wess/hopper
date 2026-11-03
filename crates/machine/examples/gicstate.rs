#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "support/recordfile.rs"]
mod files;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn main() -> anyhow::Result<()> {
  use anyhow::ensure;
  use machine::hypervisor::{
    self as hv,
    gic::{self, state},
  };
  use std::{
    fs::Permissions,
    io::{Read, Write},
    os::unix::fs::PermissionsExt,
  };
  use zeroize::Zeroizing;

  unsafe extern "C" {
    fn hv_gic_set_distributor_reg(register: u16, value: u64) -> i32;
  }
  let set = |register, value| {
    let result = unsafe { hv_gic_set_distributor_reg(register, value) };
    ensure!(result == 0, "Set GIC diagnostic register: {result}");
    Ok::<_, anyhow::Error>(())
  };
  let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
  ensure!(
    arguments.is_empty()
      || (arguments.len() == 2 && (arguments[0] == "--save" || arguments[0] == "--restore")),
    "Usage: gicstate [--save|--restore PRIVATE_DIRECTORY]"
  );
  let directory = tempfile::tempdir()?;
  std::fs::set_permissions(directory.path(), Permissions::from_mode(0o700))?;
  let root = arguments
    .get(1)
    .map(std::path::Path::new)
    .unwrap_or(directory.path());
  if arguments.first().is_none_or(|mode| mode != "--restore") {
    let vm = hv::create()?;
    let gic = gic::create_msi(&vm, 0x08000000, 0x0a000000, 0x30000000, 64, 32)?;
    let mut cpu = hv::cpu(&vm)?;
    hv::affinity(&mut cpu, 0)?;
    set(0, 0x12)?;
    set(0x104, 2)?;
    set(0x108, 1)?;
    set(0x420, 0x80706050)?;
    gic::signal(&gic, 33, true)?;
    gic::message(&gic, 0x30000040, 64)?;
    ensure!(
      gic::read(&gic, 0x204)? & 2 != 0,
      "Source SPI is not pending"
    );
    ensure!(
      gic::read(&gic, 0x208)? & 1 != 0,
      "Source MSI is not pending"
    );
    let saved = state::capture(&gic)?;
    let mut file = files::create(root, "interrupts.hgi")?;
    file.write_all(&state::wire::encode(&saved))?;
    files::sync(root, &file)?;
  }
  // no source hardware or captured state survives this point.
  if arguments.first().is_some_and(|mode| mode == "--save") {
    println!("Private GIC record synced; source hardware and captured state destroyed");
    return Ok(());
  }
  let mut bytes = Zeroizing::new(Vec::new());
  files::reader(root, "interrupts.hgi", state::wire::MAX as u64)?
    .take(state::wire::MAX as u64 + 1)
    .read_to_end(&mut bytes)?;
  let saved = state::wire::decode(&bytes)?;
  drop(bytes);
  let vm = hv::create()?;
  let gic = gic::create_msi(&vm, 0x08000000, 0x0a000000, 0x30000000, 64, 32)?;
  let mut cpu = hv::cpu(&vm)?;
  hv::affinity(&mut cpu, 0)?;
  ensure!(gic::read(&gic, 0x204)? & 2 == 0 && gic::read(&gic, 0x208)? & 1 == 0);
  state::restore(&gic, &saved)?;
  ensure!(gic::read(&gic, 0x104)? & 2 != 0 && gic::read(&gic, 0x108)? & 1 != 0);
  ensure!(
    gic::read(&gic, 0x420)? == 0x80706050,
    "GIC priorities changed"
  );
  ensure!(
    gic::read(&gic, 0x204)? & 2 != 0,
    "Restored SPI is not pending"
  );
  ensure!(
    gic::read(&gic, 0x208)? & 1 != 0,
    "Restored MSI is not pending"
  );
  set(0x288, 1)?;
  ensure!(
    gic::read(&gic, 0x208)? & 1 == 0,
    "Restored MSI cannot be cleared"
  );
  gic::message(&gic, 0x30000040, 64)?;
  ensure!(
    gic::read(&gic, 0x208)? & 1 != 0,
    "Restored MSI cannot be signalled"
  );
  println!("Private GIC record survives source teardown; fresh hardware preserves enabled SPI/MSI, pending state and priorities, and accepts clear/re-signal");
  Ok(())
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("The native hypervisor requires Apple silicon macOS")
}
