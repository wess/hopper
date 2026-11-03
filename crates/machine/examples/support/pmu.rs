use machine::hypervisor::{self as hv, state::clock};

// these are observed writable fields, not an inventory of every possible PMU register.
const SETTINGS: [(u16, u64); 7] = [
  (0xdce0, 0x41),
  (0xdce1, 1 << 31),
  (0xdce5, 7),
  (0xdcf0, 15),
  (0xc4f1, 1 << 31),
  (0xdf7f, 0xc8000000),
  (0xdcf3, 1 << 31),
];

pub(super) fn probe(
  vm: &hv::Vm,
  gic: &hv::gic::Gic<'_>,
  memory: &hv::Memory<'_>,
  mut source: hv::Cpu<'_>,
) -> anyhow::Result<hv::pmu::State> {
  for &(register, value) in &SETTINGS {
    super::access(&mut source, register, Some(value))?;
    anyhow::ensure!(
      super::access(&mut source, register, None)? == value,
      "PMU setting 0x{register:04x} was not retained"
    );
  }
  let seed = 0x1234000000000000;
  super::access(&mut source, 0xdce8, Some(seed))?;

  // each CPU is created, used and destroyed on its own owner thread.
  let factory = hv::factory(vm);
  std::thread::scope(|scope| {
    scope
      .spawn(move || -> anyhow::Result<()> {
        let mut other = hv::create_cpu(factory)?;
        hv::affinity(&mut other, 1)?;
        for &(register, value) in &SETTINGS {
          anyhow::ensure!(
            super::access(&mut other, register, None)? != value,
            "Fresh CPU inherited PMU setting 0x{register:04x}"
          );
        }
        let other_seed = 0x5678000000000000;
        super::access(&mut other, 0xdce8, Some(other_seed))?;
        let cycle = super::access(&mut other, 0xdce8, None)?;
        anyhow::ensure!((other_seed..other_seed + (1 << 32)).contains(&cycle));
        Ok(())
      })
      .join()
      .map_err(|_| anyhow::anyhow!("Secondary PMU owner panicked"))?
  })?;
  for &(register, value) in &SETTINGS {
    anyhow::ensure!(super::access(&mut source, register, None)? == value);
  }
  let cycle = super::access(&mut source, 0xdce8, None)?;
  anyhow::ensure!((seed..seed + (1 << 32)).contains(&cycle));
  println!("PMU settings and cycle values remain independent between two CPU owners");

  let host = clock::ticks();
  let frozen = clock::capture_at(&source, host)?;
  let before = super::access(&mut source, 0xdce8, None)?;
  clock::restore(&mut source, &frozen, host.wrapping_add(1 << 40))?;
  let shifted = super::access(&mut source, 0xdce8, None)?;
  clock::restore(&mut source, &frozen, host)?;
  anyhow::ensure!(
    shifted.wrapping_sub(before) < 1 << 32,
    "PMU cycle behavior changed with the architectural timer offset"
  );
  println!("A 2^40-tick timer-offset shift does not rebase the PMU cycle counter");

  super::access(&mut source, 0xc510, Some(0xff))?;
  super::access(
    &mut source,
    0xc102,
    Some((2 << 32) | (1 << 23) | (3 << 12) | (1 << 10) | (1 << 8) | 25),
  )?;
  super::access(&mut source, 0xc100, Some(0x40001000))?;
  super::access(&mut source, 0xc080, Some(1))?;
  let checkpoint = hv::state::capture(&source)?;
  let interrupts = hv::gic::state::capture(gic)?;
  let code = hv::pmu::prepare(vm, 0x3fff0000)?;
  anyhow::ensure!(
    hv::pmu::prepare(vm, 0x40000000).is_err(),
    "PMU code replaced guest RAM"
  );
  let pmu = code.token().capture(&mut source)?;
  anyhow::ensure!(
    hv::state::capture(&source)? == checkpoint,
    "PMU capture changed CPU context"
  );
  anyhow::ensure!(
    hv::gic::state::capture(gic)?.bytes() == interrupts.bytes(),
    "PMU capture changed interrupt state"
  );
  drop(source);
  let mut restored = hv::cpu(vm)?;
  hv::affinity(&mut restored, 0)?;
  hv::state::restore(&mut restored, &checkpoint)?;
  let mut missing = 0;
  for &(register, expected) in &SETTINGS {
    let observed = super::access(&mut restored, register, None)?;
    missing += usize::from(observed != expected);
    println!("Restored PMU 0x{register:04x}: expected=0x{expected:x}, observed=0x{observed:x}");
  }
  let recovered = super::access(&mut restored, 0xdce8, None)?;
  println!(
    "Architectural restoration: {missing}/{} PMU settings differ; saved cycle seed=0x{seed:x}, fresh cycle=0x{recovered:x}",
    SETTINGS.len()
  );
  let context = hv::state::capture(&restored)?;
  code.token().restore(&mut restored, &pmu)?;
  anyhow::ensure!(
    hv::state::capture(&restored)? == context,
    "PMU restore changed CPU context"
  );
  let interrupts = hv::gic::state::capture(gic)?;
  code.token().restore(&mut restored, &pmu)?;
  anyhow::ensure!(
    hv::gic::state::capture(gic)?.bytes() == interrupts.bytes(),
    "PMU restore changed interrupt state"
  );
  for &(register, expected) in &SETTINGS {
    anyhow::ensure!(
      super::access(&mut restored, register, None)? == expected,
      "PMU setting 0x{register:04x} was not restored"
    );
  }
  let recovered = super::access(&mut restored, 0xdce8, None)?;
  anyhow::ensure!(
    (seed..seed + (1 << 32)).contains(&recovered),
    "PMU cycle seed was not restored"
  );
  println!("Fixed PMU access restored all seven settings and the cycle epoch without changing architectural CPU context");
  let mut sentinel = [0; 8];
  hv::read(memory, 0x3ff8, &mut sentinel)?;
  anyhow::ensure!(u64::from_le_bytes(sentinel) == 0x123456789abcdef0);
  println!("MMU-enabled context, unmapped scratch virtual address, interrupt state and guest RAM isolation passed");
  Ok(pmu)
}

pub(super) fn recreate(
  vm: &hv::Vm,
  factory: hv::CpuFactory<'_>,
  saved: &hv::pmu::State,
  compatible: bool,
) -> anyhow::Result<()> {
  let code = hv::pmu::prepare(vm, 0x3fff0000)?;
  let mut cpu = hv::create_cpu(factory)?;
  hv::affinity(&mut cpu, 0)?;
  hv::set(&mut cpu, 0, 0xdeadbeef)?;
  if compatible {
    hv::affinity(&mut cpu, 1)?;
    let foreign = hv::state::capture(&cpu)?;
    anyhow::ensure!(
      code.token().restore(&mut cpu, saved).is_err(),
      "PMU restoration admitted another CPU affinity"
    );
    anyhow::ensure!(
      hv::state::capture(&cpu)? == foreign,
      "Rejected PMU affinity changed CPU context"
    );
    hv::affinity(&mut cpu, 0)?;
    println!("Foreign PMU affinity rejected with architectural context intact");
  }
  let before = hv::state::capture(&cpu)?;
  let restored = code.token().restore(&mut cpu, saved);
  anyhow::ensure!(
    hv::state::capture(&cpu)? == before,
    "PMU reconstruction changed CPU context"
  );
  if !compatible {
    anyhow::ensure!(restored.is_err(), "Incompatible PMU features were admitted");
    anyhow::ensure!(
      code.token().capture(&mut cpu).is_err(),
      "Unsupported PMU features were captured"
    );
    anyhow::ensure!(
      hv::state::capture(&cpu)? == before,
      "Rejected capture changed CPU context"
    );
    println!("Incompatible PMU restoration and capture rejected with architectural context intact");
    return Ok(());
  }
  restored?;
  for &(register, expected) in &SETTINGS {
    anyhow::ensure!(super::access(&mut cpu, register, None)? == expected);
  }
  let cycle = super::access(&mut cpu, 0xdce8, None)?;
  let seed = 0x1234000000000000;
  anyhow::ensure!((seed..seed + (1 << 32)).contains(&cycle));
  println!("Independent PMU state survived full VM teardown and reconstruction");
  Ok(())
}
