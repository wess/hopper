#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn main() -> anyhow::Result<()> {
  use machine::hypervisor::{
    self as hv,
    state::{self, clock},
  };
  let mrs = |register: u16, target: u32| 0xd5300000 | (u32::from(register & 0x7fff) << 5) | target;
  let msr = |register: u16, source: u32| 0xd5100000 | (u32::from(register & 0x7fff) << 5) | source;
  let program = [
    mrs(0xdf01, 0),
    mrs(0xdf02, 1),
    mrs(0xdf00, 4),
    0xd4000002,
    msr(0xdf12, 2),
    msr(0xdf1a, 3),
    msr(0xdf11, 5),
    msr(0xdf19, 5),
    0xd4000002,
    mrs(0xdf01, 0),
    mrs(0xdf02, 1),
    mrs(0xdf11, 6),
    mrs(0xdf19, 7),
    mrs(0xdf12, 8),
    mrs(0xdf1a, 9),
    0xd4000002,
  ]
  .into_iter()
  .flat_map(u32::to_le_bytes)
  .collect::<Vec<_>>();
  let (saved, frozen, virtual_before, frequency) = {
    let vm = hv::create()?;
    let _gic = hv::gic::create(&vm, 0x08000000, 0x0a000000)?;
    let mut memory = hv::memory(&vm, 0x40000000, 0x4000)?;
    hv::write(&mut memory, 0, &program)?;
    hv::protect(&memory, 5)?;
    let mut cpu = hv::cpu(&vm)?;
    hv::affinity(&mut cpu, 0)?;
    hv::enter(&mut cpu, 0x40000000)?;
    let before = clock::ticks();
    checkpoint(&mut cpu, 0x40000010)?;
    let after = clock::ticks();
    let physical = hv::get(&cpu, 0)?;
    let virtual_before = hv::get(&cpu, 1)?;
    let frequency = hv::get(&cpu, 4)?;
    anyhow::ensure!(
      (before..=after).contains(&physical),
      "Guest physical counter uses a different host epoch"
    );
    anyhow::ensure!(frequency > 0 && frequency <= 10_000_000_000);
    hv::set(&mut cpu, 2, physical + frequency / 2)?;
    hv::set(&mut cpu, 3, virtual_before + frequency / 2)?;
    hv::set(&mut cpu, 5, 1)?;
    checkpoint(&mut cpu, 0x40000024)?;
    let frozen = clock::capture_at(&cpu, clock::ticks())?;
    (state::capture(&cpu)?, frozen, virtual_before, frequency)
  };
  // longer than the enabled timers' original deadlines.
  std::thread::sleep(std::time::Duration::from_millis(750));
  let vm = hv::create()?;
  let _gic = hv::gic::create(&vm, 0x08000000, 0x0a000000)?;
  let mut memory = hv::memory(&vm, 0x40000000, 0x4000)?;
  hv::write(&mut memory, 0, &program)?;
  hv::protect(&memory, 5)?;
  let mut cpu = hv::cpu(&vm)?;
  hv::affinity(&mut cpu, 0)?;
  let restore_tick = clock::ticks();
  state::restore_frozen(&mut cpu, &saved, &frozen, restore_tick)?;
  checkpoint(&mut cpu, 0x40000040)?;
  println!("restored: host={restore_tick}, physical={}, virtual={}, physical_deadline={}, virtual_deadline={}, frequency={frequency}", hv::get(&cpu, 0)?, hv::get(&cpu, 1)?, hv::get(&cpu, 8)?, hv::get(&cpu, 9)?);
  let virtual_after = hv::get(&cpu, 1)?;
  anyhow::ensure!(
    virtual_after >= virtual_before && virtual_after - virtual_before < frequency / 4,
    "Guest virtual time included the suspended interval"
  );
  anyhow::ensure!(
    hv::get(&cpu, 6)? & 5 == 1,
    "Restored physical timer expired during suspension"
  );
  anyhow::ensure!(
    hv::get(&cpu, 7)? & 5 == 1,
    "Restored virtual timer expired during suspension"
  );
  std::thread::sleep(std::time::Duration::from_millis(550));
  hv::set(&mut cpu, 31, 0x40000024)?;
  checkpoint(&mut cpu, 0x40000040)?;
  println!(
    "later: host={}, physical={}, virtual={}, physical_ctl={}, virtual_ctl={}",
    clock::ticks(),
    hv::get(&cpu, 0)?,
    hv::get(&cpu, 1)?,
    hv::get(&cpu, 6)?,
    hv::get(&cpu, 7)?
  );
  anyhow::ensure!(
    hv::get(&cpu, 6)? & 5 == 5,
    "Restored physical timer never expired"
  );
  anyhow::ensure!(
    hv::get(&cpu, 7)? & 5 == 5,
    "Restored virtual timer never expired"
  );
  println!("Frozen clocks: 750 ms suspension excluded; both enabled 500 ms timers retain their delay and then expire across VM recreation");
  Ok(())
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn checkpoint(cpu: &mut machine::hypervisor::Cpu<'_>, pc: u64) -> anyhow::Result<()> {
  use machine::{arm, hypervisor as hv};
  let exit = hv::bounded(cpu, std::time::Duration::from_secs(2), hv::run)?;
  let hv::Exit::Exception { syndrome, .. } = exit else {
    anyhow::bail!("Clock diagnostic did not reach its checkpoint: {exit:?}");
  };
  anyhow::ensure!(arm::decode(syndrome) == arm::Trap::Hypercall(0) && hv::get(cpu, 31)? == pc);
  Ok(())
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("The native hypervisor requires Apple silicon macOS")
}
