#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn main() -> anyhow::Result<()> {
  use anyhow::{ensure, Context};
  use machine::{
    arm,
    devices::{
      pci,
      virtio::{block, queue},
    },
    hypervisor as hv,
  };
  use std::{io::Read, path::PathBuf};

  let path = PathBuf::from(
    std::env::args()
      .nth(1)
      .context("Provide a sector-aligned test disk")?,
  );
  let mut disk = block::open(&path, true, [0; 20])?;
  let mut expected = [0; 8];
  std::fs::File::open(&path)?.read_exact(&mut expected)?;
  let vm = hv::create()?;
  let gic = hv::gic::create(&vm, 0x08000000, 0x0a000000)?;
  let base = 0x40000000;
  let mut ram = hv::memory(&vm, base, 0x20000)?;
  for (index, offset, length, flags, next) in [
    (0, 0x1000, 16u32, 1u16, 1u16),
    (1, 0x2000, 512, 3, 2),
    (2, 0x4000, 1, 2, 0),
  ] {
    let mut descriptor = Vec::new();
    descriptor.extend((base + offset).to_le_bytes());
    descriptor.extend(length.to_le_bytes());
    descriptor.extend(flags.to_le_bytes());
    descriptor.extend(next.to_le_bytes());
    hv::write(&mut ram, index * 16, &descriptor)?;
  }
  hv::write(&mut ram, 0x4000, &[0xff])?;
  let mut queue = queue::create(&mut ram, 8, base, base + 0x100, base + 0x200)?;
  // strh w0,[x1]; dmb ish; hvc #0; ldrb w2,[x3]; ldr x4,[x5]; hvc #0.
  let code: Vec<u8> = [
    0x79000020u32,
    0xd5033bbf,
    0xd4000002,
    0x39400062,
    0xf94000a4,
    0xd4000002,
  ]
  .into_iter()
  .flat_map(u32::to_le_bytes)
  .collect();
  hv::write(&mut ram, 0x10000, &code)?;
  let mut cpu = hv::cpu(&vm)?;
  hv::set(&mut cpu, 0, 1)?;
  hv::set(&mut cpu, 1, base + 0x102)?;
  hv::set(&mut cpu, 3, base + 0x4000)?;
  hv::set(&mut cpu, 5, base + 0x2000)?;
  hv::enter(&mut cpu, base + 0x10000)?;
  hv::bounded(&mut cpu, std::time::Duration::from_secs(1), |cpu| {
    let hv::Exit::Exception { syndrome, .. } = hv::run(cpu)? else {
      anyhow::bail!("Guest did not publish its storage request");
    };
    ensure!(
      arm::decode(syndrome) == arm::Trap::Hypercall(0),
      "Guest missed its queue notification"
    );
    let chain = queue::pop(&mut queue, &ram)?.context("Guest did not make a buffer available")?;
    let written = block::execute(&mut disk, &mut ram, &chain)?;
    ensure!(
      written == 513,
      "Disk did not return one sector and its status"
    );
    ensure!(
      queue::complete(&mut queue, &mut ram, &chain, written)?,
      "Guest unexpectedly suppressed its interrupt"
    );
    let irq = pci::interrupt(0, 1)?;
    hv::gic::signal(&gic, irq, true)?;
    ensure!(
      hv::gic::read(&gic, 0x204)? & (1 << (irq - 32)) != 0,
      "Storage interrupt did not become pending"
    );
    let hv::Exit::Exception { syndrome, .. } = hv::run(cpu)? else {
      anyhow::bail!("Guest did not consume its storage response");
    };
    ensure!(
      arm::decode(syndrome) == arm::Trap::Hypercall(0),
      "Guest missed its response checkpoint"
    );
    ensure!(hv::get(cpu, 2)? == 0, "Guest received a disk error");
    ensure!(
      hv::get(cpu, 4)? == u64::from_le_bytes(expected),
      "Guest read the wrong disk data"
    );
    hv::gic::signal(&gic, irq, false)?;
    Ok(())
  })?;
  println!("Native guest queue submission, disk read, completion and interrupt signal passed");
  Ok(())
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("The native storage probe requires Apple silicon macOS")
}
