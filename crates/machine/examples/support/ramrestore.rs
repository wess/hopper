use anyhow::ensure;
use machine::{
  arm,
  hypervisor::{self as hv, ram},
};
use std::time::{Duration, Instant};

const BASE: u64 = 0x40000000;
const PAGE: usize = 0x4000;

fn resident() -> anyhow::Result<u64> {
  let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
  ensure!(unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } == 0);
  Ok(unsafe { usage.assume_init() }.ru_maxrss as u64)
}

pub fn run() -> anyhow::Result<()> {
  for size in [1usize << 30, 8usize << 30, 64usize << 30] {
    let peak = resident()?;
    let saved = {
      let vm = hv::create()?;
      let outside = hv::memory(&vm, 1u64 << 40, PAGE)
        .err()
        .ok_or_else(|| anyhow::anyhow!("Memory outside configured address range was admitted"))?;
      ensure!(outside.to_string().contains("physical address range"));
      let overflow = hv::memory(&vm, !(PAGE as u64 - 1), PAGE)
        .err()
        .ok_or_else(|| anyhow::anyhow!("Overflowing memory range was admitted"))?;
      ensure!(overflow.to_string().contains("physical address range"));
      let mut memory = hv::memory(&vm, BASE, size)?;
      let code = [0xf9400022u32, 0xd4000002, 0xf9000020, 0xd4000002]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
      hv::write(&mut memory, 0, &code)?;
      hv::write(&mut memory, PAGE, &22u64.to_le_bytes())?;
      hv::write(&mut memory, size - PAGE, &33u64.to_le_bytes())?;
      let now = Instant::now();
      let saved = ram::capture(&memory)?;
      println!("{} GiB sparse RAM capture: {:?}", size >> 30, now.elapsed());
      saved
    };
    let vm = hv::create()?;
    let _gic = hv::gic::create(&vm, 0x08000000, 0x0a000000)?;
    let now = Instant::now();
    let mut restored = ram::restore(&vm, &saved)?;
    println!(
      "{} GiB sparse RAM COW restore: {:?}",
      size >> 30,
      now.elapsed()
    );
    ensure!(
      resident()?.saturating_sub(peak) < 128 * 1024 * 1024,
      "Sparse restoration materialized too many pages; stop before larger cases"
    );
    let mut cpu = hv::cpu(&vm)?;
    for (offset, expected) in [(PAGE, 22), (size - PAGE, 33), (size / 2, 0)] {
      hv::enter(&mut cpu, BASE)?;
      hv::set(&mut cpu, 1, BASE + offset as u64)?;
      let exit = hv::bounded(&mut cpu, Duration::from_secs(5), hv::run)?;
      ensure!(
        matches!(exit, hv::Exit::Exception { syndrome, .. } if arm::decode(syndrome) == arm::Trap::Hypercall(0))
      );
      ensure!(
        hv::get(&cpu, 2)? == expected,
        "Guest read incorrect restored sparse memory"
      );
      hv::enter(&mut cpu, BASE + 8)?;
      hv::set(&mut cpu, 0, 99)?;
      hv::set(&mut cpu, 1, BASE + offset as u64)?;
      let exit = hv::bounded(&mut cpu, Duration::from_secs(5), hv::run)?;
      ensure!(
        matches!(exit, hv::Exit::Exception { syndrome, .. } if arm::decode(syndrome) == arm::Trap::Hypercall(0))
      );
      let mut bytes = [0; 8];
      saved.read(offset, &mut bytes)?;
      ensure!(
        bytes == expected.to_le_bytes(),
        "Guest write changed captured sparse memory"
      );
    }
    hv::write(&mut restored, size / 2, &101u64.to_le_bytes())?;
    let mut zero = [1; 8];
    saved.read(size / 2, &mut zero)?;
    ensure!(zero == [0; 8]);
    let next = ram::capture(&restored)?;
    drop(cpu);
    drop(restored);
    let restored = ram::restore(&vm, &next)?;
    drop(next);
    drop(saved);
    let mut bytes = [0; 8];
    hv::read(&restored, size / 2, &mut bytes)?;
    ensure!(
      bytes == 101u64.to_le_bytes(),
      "Recovered COW pages lost their backing owner"
    );
    println!(
      "{} GiB sparse restore peak RSS growth: {} bytes",
      size >> 30,
      resident()?.saturating_sub(peak)
    );
  }
  println!("Sparse 1/8/64 GiB restoration: boundary/hole guest reads, isolated guest/host writes, recapture and capture-owner teardown passed");
  Ok(())
}
