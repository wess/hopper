#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "support/cpufile.rs"]
mod record;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn main() -> anyhow::Result<()> {
  use machine::{
    arm,
    hypervisor::{self as hv, state},
  };
  use std::os::unix::fs::PermissionsExt;

  // seed thread and stack state, checkpoint, then read it after VM recreation.
  let program = [
    0xd51bd040u32,
    0xd518d081,
    0xd5184102,
    0xd5100085,
    0xd4000002,
    0xd53bd041,
    0xd5300086,
    0x91000400,
    0xd4000002,
  ]
  .into_iter()
  .flat_map(u32::to_le_bytes)
  .collect::<Vec<_>>();
  let args = std::env::args_os().skip(1).collect::<Vec<_>>();
  anyhow::ensure!(
    args.is_empty() || (args.len() == 2 && (args[0] == "--save" || args[0] == "--restore")),
    "Use no arguments, --save PRIVATE_DIRECTORY or --restore PRIVATE_DIRECTORY"
  );
  let temporary = if args.is_empty() {
    Some(tempfile::tempdir()?)
  } else {
    None
  };
  let root = temporary
    .as_ref()
    .map(|root| root.path())
    .unwrap_or_else(|| std::path::Path::new(&args[1]));
  if temporary.is_some() {
    std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700))?;
  }
  if args.is_empty() || args[0] == "--save" {
    let vm = hv::create()?;
    let _gic = hv::gic::create(&vm, 0x08000000, 0x0a000000)?;
    let mut memory = hv::memory(&vm, 0x40000000, 0x4000)?;
    hv::write(&mut memory, 0, &program)?;
    let mut cpu = hv::cpu(&vm)?;
    hv::affinity(&mut cpu, 0)?;
    for index in 0..31 {
      hv::set(&mut cpu, index, 0x1234567800000000 + u64::from(index))?;
    }
    hv::set(&mut cpu, 5, 0x40001000)?;
    hv::set(&mut cpu, 32, 0x400000)?;
    hv::set(&mut cpu, 33, 0x1f)?;
    for index in 0..32 {
      let bytes = std::array::from_fn(|byte| (index * 17 + byte as u32) as u8);
      state::set_vector(&mut cpu, index, &bytes)?;
    }
    hv::enter(&mut cpu, 0x40000000)?;
    let hv::Exit::Exception { syndrome, .. } = checkpoint(&mut cpu)? else {
      anyhow::bail!("CPU state guest did not reach its first checkpoint");
    };
    anyhow::ensure!(arm::decode(syndrome) == arm::Trap::Hypercall(0));
    anyhow::ensure!(hv::get(&cpu, 0)? == 0x1234567800000000);
    anyhow::ensure!(hv::get(&cpu, 31)? == 0x40000014);
    anyhow::ensure!(state::vector(&cpu, 32).is_err());
    anyhow::ensure!(state::set_vector(&mut cpu, 32, &[0; 16]).is_err());
    record::save(root, &state::capture(&cpu)?)?;
  }
  if !args.is_empty() && args[0] == "--save" {
    println!("Private CPU record synced; source hardware and captured state destroyed");
    return Ok(());
  }
  // reopen only after the original CPU, VM and captured state have been destroyed.
  let saved = record::open(root)?;
  {
    // without the GIC factory's PMU feature configuration, restoration must reject
    // before touching even the first general register.
    let vm = hv::create()?;
    let mut cpu = hv::cpu(&vm)?;
    hv::set(&mut cpu, 0, 0x1234)?;
    anyhow::ensure!(state::restore(&mut cpu, &saved).is_err());
    anyhow::ensure!(hv::get(&cpu, 0)? == 0x1234);
  }
  let vm = hv::create()?;
  let _gic = hv::gic::create(&vm, 0x08000000, 0x0a000000)?;
  let mut memory = hv::memory(&vm, 0x40000000, 0x4000)?;
  hv::write(&mut memory, 0, &program)?;
  let mut cpu = hv::cpu(&vm)?;
  hv::affinity(&mut cpu, 0)?;
  state::restore(&mut cpu, &saved)?;
  anyhow::ensure!(
    state::capture(&cpu)? == saved,
    "CPU register state changed across recreation"
  );
  let hv::Exit::Exception { syndrome, .. } = checkpoint(&mut cpu)? else {
    anyhow::bail!("Restored CPU did not reach its next checkpoint");
  };
  anyhow::ensure!(arm::decode(syndrome) == arm::Trap::Hypercall(0));
  anyhow::ensure!(hv::get(&cpu, 0)? == 0x1234567800000001);
  anyhow::ensure!(hv::get(&cpu, 1)? == 0x1234567800000000);
  anyhow::ensure!(hv::get(&cpu, 6)? == 0x40001000);
  anyhow::ensure!(hv::get(&cpu, 31)? == 0x40000024);
  for index in 0..32 {
    let expected = std::array::from_fn(|byte| (index * 17 + byte as u32) as u8);
    anyhow::ensure!(
      state::vector(&cpu, index)? == expected,
      "Restored SIMD value changed"
    );
  }
  println!(
    "CPU state: private checksummed file reopening, general/FP/SIMD/system/debug/timer round trip, incompatible-feature rejection and continued guest execution passed"
  );
  Ok(())
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn checkpoint(cpu: &mut machine::hypervisor::Cpu<'_>) -> anyhow::Result<machine::hypervisor::Exit> {
  use machine::hypervisor as hv;
  hv::bounded(cpu, std::time::Duration::from_secs(5), hv::run)
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("The native hypervisor requires Apple silicon macOS")
}
