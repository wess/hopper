#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "support/recordfile.rs"]
mod files;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn main() -> anyhow::Result<()> {
  use anyhow::ensure;
  use machine::{
    arm,
    hypervisor::{self as hv, ram},
  };
  use std::{fs::Permissions, os::unix::fs::PermissionsExt, time::Duration};

  const BASE: u64 = 0x40000000;
  const SIZE: usize = 32 << 20;
  const FILE_LIMIT: u64 = SIZE as u64 + (SIZE as u64 / 65536) * 4 + 56;
  let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
  ensure!(
    arguments.is_empty()
      || (arguments.len() == 2 && (arguments[0] == "--save" || arguments[0] == "--restore")),
    "Usage: ramstate [--save|--restore PRIVATE_DIRECTORY]"
  );
  let directory = tempfile::tempdir()?;
  std::fs::set_permissions(directory.path(), Permissions::from_mode(0o700))?;
  let root = arguments
    .get(1)
    .map(std::path::Path::new)
    .unwrap_or(directory.path());
  if arguments.first().is_none_or(|mode| mode != "--restore") {
    let vm = hv::create()?;
    let mut memory = hv::memory(&vm, BASE, SIZE)?;
    let code = [0xf9400022u32, 0xd4000002, 0xf9000020, 0xd4000002]
      .into_iter()
      .flat_map(u32::to_le_bytes)
      .collect::<Vec<_>>();
    hv::write(&mut memory, 0, &code)?;
    hv::write(&mut memory, 0x4000, &22u64.to_le_bytes())?;
    hv::write(&mut memory, SIZE - 8, &33u64.to_le_bytes())?;
    let saved = ram::capture(&memory)?;
    hv::write(&mut memory, 0x4000, &99u64.to_le_bytes())?;
    let mut file = files::create(root, "memory.hrm")?;
    ram::wire::encode(&saved, &mut file, || Ok(()))?;
    files::sync(root, &file)?;
    ensure!(
      file.metadata()?.len() < 256 << 10,
      "Zero chunks were materialized"
    );
  }
  if arguments.first().is_some_and(|mode| mode == "--save") {
    println!("Private memory record synced; source VM and immutable mapping destroyed");
    return Ok(());
  }
  let mut file = files::reader(root, "memory.hrm", FILE_LIMIT)?;
  let saved = ram::wire::decode(&mut file, (BASE, SIZE), || Ok(()))?;
  drop(file);
  for attempt in 0..2 {
    let vm = hv::create()?;
    let _gic = hv::gic::create(&vm, 0x08000000, 0x0a000000)?;
    let mut memory = ram::restore(&vm, &saved)?;
    let mut cpu = hv::cpu(&vm)?;
    hv::affinity(&mut cpu, 0)?;
    for (offset, expected) in [(0x4000, 22u64), (SIZE / 2, 0), (SIZE - 8, 33)] {
      hv::enter(&mut cpu, BASE)?;
      hv::set(&mut cpu, 1, BASE + offset as u64)?;
      let exit = hv::bounded(&mut cpu, Duration::from_secs(2), hv::run)?;
      ensure!(matches!(exit, hv::Exit::Exception { syndrome, .. }
        if arm::decode(syndrome) == arm::Trap::Hypercall(0)));
      ensure!(
        hv::get(&cpu, 2)? == expected,
        "Guest read incorrect reopened memory"
      );
      hv::enter(&mut cpu, BASE + 8)?;
      hv::set(&mut cpu, 0, 77)?;
      let exit = hv::bounded(&mut cpu, Duration::from_secs(2), hv::run)?;
      ensure!(matches!(exit, hv::Exit::Exception { syndrome, .. }
        if arm::decode(syndrome) == arm::Trap::Hypercall(0)));
      let mut bytes = [0; 8];
      saved.read(offset, &mut bytes)?;
      ensure!(
        u64::from_le_bytes(bytes) == expected,
        "Guest write changed saved memory"
      );
    }
    hv::write(&mut memory, SIZE / 2 + 8, &101u64.to_le_bytes())?;
    let mut bytes = [1; 8];
    saved.read(SIZE / 2 + 8, &mut bytes)?;
    ensure!(bytes == [0; 8], "Host write changed saved memory");
    println!("Reopened memory attempt {attempt}: guest execution, zero/nonzero chunks and guest/host write isolation passed");
  }
  Ok(())
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("The native hypervisor requires Apple silicon macOS")
}
