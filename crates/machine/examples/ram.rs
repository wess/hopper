#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "support/ramrestore.rs"]
mod ramrestore;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn main() -> anyhow::Result<()> {
  use machine::{
    arm,
    dma::Memory as _,
    hypervisor::{self as hv, ram},
  };
  use std::time::{Duration, Instant};

  const BASE: u64 = 0x40000000;
  const PAGE: usize = 0x4000;
  const SIZE: usize = 32 * 1024 * 1024;
  let program = [
    0xf9000020u32,
    0x8b030021,
    0xeb04003f,
    0x54ffffa3,
    0xd4000002,
  ]
  .into_iter()
  .flat_map(u32::to_le_bytes)
  .collect::<Vec<_>>();
  let read = [0xf9400022u32, 0xd4000002]
    .into_iter()
    .flat_map(u32::to_le_bytes)
    .collect::<Vec<_>>();
  let (first, second) = {
    let vm = hv::create()?;
    let _gic = hv::gic::create(&vm, 0x08000000, 0x0a000000)?;
    let mut memory = hv::memory(&vm, BASE, SIZE)?;
    for offset in (PAGE..SIZE).step_by(PAGE) {
      hv::write(&mut memory, offset, &vec![0x5a; PAGE])?;
    }
    hv::write(&mut memory, 0, &program)?;
    hv::write(&mut memory, 128, &read)?;
    let mut cpu = hv::cpu(&vm)?;
    hv::affinity(&mut cpu, 0)?;
    hv::enter(&mut cpu, BASE)?;
    let mut write_guest = |value| -> anyhow::Result<()> {
      hv::set(&mut cpu, 0, value)?;
      hv::set(&mut cpu, 1, BASE + PAGE as u64)?;
      hv::set(&mut cpu, 3, PAGE as u64)?;
      hv::set(&mut cpu, 4, BASE + SIZE as u64)?;
      hv::set(&mut cpu, 31, BASE)?;
      let exit = hv::bounded(&mut cpu, Duration::from_secs(5), hv::run)?;
      anyhow::ensure!(
        matches!(exit, hv::Exit::Exception { syndrome, .. } if arm::decode(syndrome) == arm::Trap::Hypercall(0)),
        "RAM guest failed to reach its checkpoint"
      );
      Ok(())
    };
    write_guest(22)?;
    let started = Instant::now();
    let first = ram::capture(&memory)?;
    println!("32 MiB populated RAM capture: {:?}", started.elapsed());
    write_guest(33)?;
    verify(&first, 22, PAGE, SIZE)?;
    let second = ram::capture(&memory)?;
    std::thread::scope(|scope| -> anyhow::Result<()> {
      let reader = scope.spawn(|| -> anyhow::Result<()> {
        for _ in 0..3 {
          verify(&first, 22, PAGE, SIZE)?;
          verify(&second, 33, PAGE, SIZE)?;
        }
        Ok(())
      });
      for value in [55, 56, 55] {
        write_guest(value)?;
      }
      reader
        .join()
        .map_err(|_| anyhow::anyhow!("Concurrent RAM reader panicked"))??;
      Ok(())
    })?;
    verify(&first, 22, PAGE, SIZE)?;
    verify(&second, 33, PAGE, SIZE)?;
    for offset in (PAGE..SIZE).step_by(PAGE) {
      // exercise both production host write paths against every captured data page.
      let mut before = [0; 8];
      hv::read(&memory, offset, &mut before)?;
      anyhow::ensure!(before == 55u64.to_le_bytes());
      hv::write(&mut memory, offset, &44u64.to_le_bytes())?;
      memory.write(BASE + offset as u64 + 8, &66u64.to_le_bytes())?;
      let mut bytes = [0; 16];
      hv::read(&memory, offset, &mut bytes)?;
      anyhow::ensure!(&bytes[..8] == 44u64.to_le_bytes());
      anyhow::ensure!(&bytes[8..] == 66u64.to_le_bytes());
    }
    for (offset, expected) in [(0, 44), (8, 66)] {
      hv::set(&mut cpu, 1, BASE + SIZE as u64 - PAGE as u64 + offset)?;
      hv::set(&mut cpu, 31, BASE + 128)?;
      let exit = hv::bounded(&mut cpu, Duration::from_secs(5), hv::run)?;
      anyhow::ensure!(
        matches!(exit, hv::Exit::Exception { syndrome, .. } if arm::decode(syndrome) == arm::Trap::Hypercall(0))
      );
      anyhow::ensure!(
        hv::get(&cpu, 2)? == expected,
        "Guest did not observe host RAM write"
      );
    }
    verify(&first, 22, PAGE, SIZE)?;
    verify(&second, 33, PAGE, SIZE)?;
    anyhow::ensure!(first.guest() == BASE && first.size() == SIZE);
    anyhow::ensure!(first.read(SIZE, &mut [0]).is_err());
    anyhow::ensure!(first.read(usize::MAX, &mut [0]).is_err());
    anyhow::ensure!(first.read(0, &mut vec![0; 65537]).is_err());
    first.read(SIZE, &mut [])?;
    (first, second)
  };
  std::thread::scope(|scope| -> anyhow::Result<()> {
    let thread = scope.spawn(|| verify(&first, 22, PAGE, SIZE));
    verify(&second, 33, PAGE, SIZE)?;
    thread
      .join()
      .map_err(|_| anyhow::anyhow!("RAM capture reader panicked"))??;
    Ok(())
  })?;
  {
    let vm = hv::create()?;
    let _gic = hv::gic::create(&vm, 0x08000000, 0x0a000000)?;
    let started = Instant::now();
    let mut memory = ram::restore(&vm, &first)?;
    println!("32 MiB populated RAM COW restore: {:?}", started.elapsed());
    anyhow::ensure!(
      ram::restore(&vm, &first).is_err(),
      "Occupied guest range accepted a second mapping"
    );
    let mut cpu = hv::cpu(&vm)?;
    // the snapshot retains both immutable guest programs.
    hv::enter(&mut cpu, BASE + 128)?;
    hv::set(&mut cpu, 1, BASE + SIZE as u64 - PAGE as u64)?;
    let exit = hv::bounded(&mut cpu, Duration::from_secs(5), hv::run)?;
    anyhow::ensure!(
      matches!(exit, hv::Exit::Exception { syndrome, .. } if arm::decode(syndrome) == arm::Trap::Hypercall(0))
    );
    anyhow::ensure!(
      hv::get(&cpu, 2)? == 22,
      "Guest did not observe restored RAM"
    );
    // guest writes must fault into private pages while the retained capture stays immutable.
    hv::set(&mut cpu, 0, 77)?;
    hv::set(&mut cpu, 1, BASE + PAGE as u64)?;
    hv::set(&mut cpu, 3, PAGE as u64)?;
    hv::set(&mut cpu, 4, BASE + SIZE as u64)?;
    hv::set(&mut cpu, 31, BASE)?;
    let exit = hv::bounded(&mut cpu, Duration::from_secs(5), hv::run)?;
    anyhow::ensure!(
      matches!(exit, hv::Exit::Exception { syndrome, .. } if arm::decode(syndrome) == arm::Trap::Hypercall(0))
    );
    verify(&first, 22, PAGE, SIZE)?;
    verify(&second, 33, PAGE, SIZE)?;
    for offset in (PAGE..SIZE).step_by(PAGE) {
      let mut bytes = [0; 8];
      hv::read(&memory, offset, &mut bytes)?;
      anyhow::ensure!(bytes == 77u64.to_le_bytes());
      hv::write(&mut memory, offset, &88u64.to_le_bytes())?;
      memory.write(BASE + offset as u64 + 8, &99u64.to_le_bytes())?;
    }
    verify(&first, 22, PAGE, SIZE)?;
    verify(&second, 33, PAGE, SIZE)?;
    drop(cpu);
    drop(memory);
    let mut memory = ram::restore(&vm, &first)?;
    let mut bytes = [0; 8];
    hv::read(&memory, SIZE - PAGE, &mut bytes)?;
    anyhow::ensure!(bytes == 22u64.to_le_bytes());
    drop(first);
    drop(second);
    hv::write(&mut memory, SIZE - PAGE, &101u64.to_le_bytes())?;
    let mut cpu = hv::cpu(&vm)?;
    hv::enter(&mut cpu, BASE + 128)?;
    hv::set(&mut cpu, 1, BASE + SIZE as u64 - PAGE as u64)?;
    let exit = hv::bounded(&mut cpu, Duration::from_secs(5), hv::run)?;
    anyhow::ensure!(
      matches!(exit, hv::Exit::Exception { syndrome, .. } if arm::decode(syndrome) == arm::Trap::Hypercall(0))
    );
    anyhow::ensure!(
      hv::get(&cpu, 2)? == 101,
      "Dropping captured mappings invalidated recovered RAM"
    );
    drop(cpu);
    drop(memory);
  }
  ramrestore::run()?;
  println!(
    "RAM capture: guest/DMA isolation, repeated captures, bounded reads, VM-independent concurrent reads and fresh guest memory restoration passed"
  );
  Ok(())
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn verify(
  capture: &machine::hypervisor::ram::Capture,
  value: u64,
  page: usize,
  size: usize,
) -> anyhow::Result<()> {
  let mut bytes = vec![0; page];
  for offset in (page..size).step_by(page) {
    capture.read(offset, &mut bytes)?;
    anyhow::ensure!(
      bytes[..8] == value.to_le_bytes(),
      "Captured guest value changed"
    );
    anyhow::ensure!(
      bytes[8..].iter().all(|byte| *byte == 0x5a),
      "Captured page changed"
    );
  }
  Ok(())
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("The native hypervisor requires Apple silicon macOS")
}
