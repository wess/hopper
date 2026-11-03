// all probe instructions are published before execution; guest code stays immutable.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const REGISTERS: [u16; 15] = [
  0xdce0, 0xdce1, 0xdce2, 0xdce3, 0xdce5, 0xdce8, 0xdcf0, 0xc4f1, 0xc4f2, 0xdf7f, 0xdcf3, 0xc080,
  0xc102, 0xc100, 0xc510,
];

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "support/pmu.rs"]
mod preservation;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn main() -> anyhow::Result<()> {
  use machine::hypervisor as hv;

  let vm = hv::create()?;
  let gic = hv::gic::create(&vm, 0x08000000, 0x0a000000)?;
  let mut memory = hv::memory(&vm, 0x40000000, 0x4000)?;
  let mut programs = Vec::new();
  for register in REGISTERS.into_iter().chain(0xdf00..0xdf20) {
    for operation in [0xd5300000, 0xd5100000] {
      let instruction = operation | (u32::from(register & 0x7fff) << 5);
      programs.extend(
        [instruction, 0xd4000002]
          .into_iter()
          .flat_map(u32::to_le_bytes),
      );
    }
  }
  hv::write(&mut memory, 0, &programs)?;
  // a 39-bit, 4 KiB stage-one table maps only the 1 GiB guest RAM block.
  hv::write(&mut memory, 0x1008, &0x40000701u64.to_le_bytes())?;
  hv::write(&mut memory, 0x3ff8, &0x123456789abcdef0u64.to_le_bytes())?;
  hv::protect(&memory, 5)?;
  let mut cpu = hv::cpu(&vm)?;
  hv::affinity(&mut cpu, 0)?;
  let control = access(&mut cpu, 0xdce0, None)?;
  let counters = ((control >> 11) & 31) as u16;
  println!("Guest PMU: PMCR=0x{control:x}, {counters} event counters");
  // keep counters and interrupts disabled while probing deterministic saved values.
  access(&mut cpu, 0xdce2, Some(u32::MAX.into()))?;
  access(&mut cpu, 0xc4f2, Some(u32::MAX.into()))?;
  access(&mut cpu, 0xdce0, Some(control & !1))?;
  let seed = 0x1234567800000000;
  access(&mut cpu, 0xdce8, Some(seed))?;
  let cycle = access(&mut cpu, 0xdce8, None)?;
  anyhow::ensure!(
    (seed..seed + (1 << 40)).contains(&cycle),
    "Cycle counter write was not retained"
  );
  println!(
    "Guest PMU: cycle counter write retained with {} elapsed cycles",
    cycle - seed
  );
  std::thread::sleep(std::time::Duration::from_millis(100));
  let later = access(&mut cpu, 0xdce8, None)?;
  anyhow::ensure!(later >= cycle, "Cycle counter moved backwards while held");
  println!(
    "Guest PMU: {} cycles advanced across a 100 ms owner hold",
    later - cycle
  );
  for index in 0..counters {
    let register = 0xdf00 + index;
    let value = 0x12340000 + u64::from(index);
    access(&mut cpu, register, Some(value))?;
    anyhow::ensure!(access(&mut cpu, register, None)? == value);
  }
  for (name, register) in [
    ("PMCR", 0xdce0),
    ("PMCNTENSET", 0xdce1),
    ("PMOVSCLR", 0xdce3),
    ("PMSELR", 0xdce5),
    ("PMUSERENR", 0xdcf0),
    ("PMINTENSET", 0xc4f1),
    ("PMCCFILTR", 0xdf7f),
  ] {
    match access(&mut cpu, register, None) {
      Ok(value) => println!("Guest PMU {name}: 0x{value:x}"),
      Err(error) => println!("Guest PMU {name}: unavailable ({error})"),
    }
  }
  println!(
    "Guest PMU cycle write/read passed; {counters} event counter write/read checks completed"
  );
  let saved = preservation::probe(&vm, &gic, &memory, cpu)?;
  drop(memory);
  drop(vm);
  {
    let vm = hv::create()?;
    let factory = hv::factory(&vm);
    let _gic = hv::gic::create(&vm, 0x08000000, 0x0a000000)?;
    preservation::recreate(&vm, factory, &saved, false)?;
  }
  let vm = hv::create()?;
  let _gic = hv::gic::create(&vm, 0x08000000, 0x0a000000)?;
  let mut memory = hv::memory(&vm, 0x40000000, 0x4000)?;
  hv::write(&mut memory, 0, &programs)?;
  hv::protect(&memory, 5)?;
  preservation::recreate(&vm, hv::factory(&vm), &saved, true)?;
  Ok(())
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn access(
  cpu: &mut machine::hypervisor::Cpu<'_>,
  register: u16,
  value: Option<u64>,
) -> anyhow::Result<u64> {
  use machine::{arm, hypervisor as hv};
  let index = REGISTERS
    .iter()
    .position(|&candidate| candidate == register)
    .or_else(|| {
      (0xdf00..0xdf20)
        .contains(&register)
        .then(|| REGISTERS.len() + usize::from(register - 0xdf00))
    })
    .ok_or_else(|| anyhow::anyhow!("Unknown PMU probe register"))?;
  let address = 0x40000000 + index as u64 * 16 + if value.is_some() { 8 } else { 0 };
  if let Some(value) = value {
    hv::set(cpu, 0, value)?;
  }
  hv::enter(cpu, address)?;
  let exit = hv::bounded(cpu, std::time::Duration::from_secs(2), hv::run)?;
  let hv::Exit::Exception { syndrome, .. } = exit else {
    anyhow::bail!("PMU register 0x{register:04x} did not reach its checkpoint: {exit:?}");
  };
  anyhow::ensure!(
    arm::decode(syndrome) == arm::Trap::Hypercall(0) && hv::get(cpu, 31)? == address + 8,
    "PMU register 0x{register:04x} trapped: {:?}, PC=0x{:x}",
    arm::decode(syndrome),
    hv::get(cpu, 31)?,
  );
  hv::get(cpu, 0)
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("The native hypervisor requires Apple silicon macOS")
}
