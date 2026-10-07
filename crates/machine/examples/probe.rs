#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn main() -> anyhow::Result<()> {
  use machine::{arm, hypervisor as hv};

  let vm = hv::create()?;
  for (address, first, count) in [
    (0x30000001, 64, 32),
    (0x30000000, 64, 0),
    (0x08000000, 64, 32),
    (0x0a000000, 64, 32),
    (0x30000000, 64, u32::MAX),
  ] {
    anyhow::ensure!(
      hv::gic::create_msi(&vm, 0x08000000, 0x0a000000, address, first, count).is_err(),
      "Invalid MSI configuration was accepted"
    );
  }
  let gic = hv::gic::create_msi(&vm, 0x08000000, 0x0a000000, 0x30000000, 64, 32)?;
  let mut memory = hv::memory(&vm, 0x40000000, 0x4000)?;
  // mov x1, #0x1000; mov x0, #42; str x0, [x1]; ldr x2, [x1]; hvc #0.
  // the unmapped address exercises the same traps used by virtual devices.
  hv::write(
    &mut memory,
    0,
    &[
      0x01, 0x00, 0x82, 0xd2, 0x40, 0x05, 0x80, 0xd2, 0x20, 0x00, 0x00, 0xf9, 0x22, 0x00, 0x40,
      0xf9, 0x02, 0x00, 0x00, 0xd4,
    ],
  )?;
  let mut cpu = hv::cpu(&vm)?;
  hv::affinity(&mut cpu, 0)?;
  hv::enter(&mut cpu, 0x40000000)?;
  let mut device = 0;
  for write in [true, false] {
    let hv::Exit::Exception {
      syndrome,
      physical_address,
      ..
    } = hv::run(&mut cpu)?
    else {
      anyhow::bail!("Guest did not trap its device access");
    };
    let arm::Trap::DataAbort(Some(access)) = arm::decode(syndrome) else {
      anyhow::bail!("Device access lacks a valid instruction syndrome: 0x{syndrome:x}");
    };
    anyhow::ensure!(
      physical_address == 0x1000 && access.bytes == 8 && access.write == write,
      "Guest trapped the wrong device access"
    );
    if access.write {
      device = hv::get(&cpu, access.register.into())?;
    } else {
      hv::set(&mut cpu, access.register.into(), device)?;
    }
    let next = hv::get(&cpu, 31)?
      .checked_add(4)
      .ok_or_else(|| anyhow::anyhow!("Guest PC overflow"))?;
    hv::set(&mut cpu, 31, next)?;
  }
  match hv::run(&mut cpu)? {
    hv::Exit::Exception { syndrome, .. } if arm::decode(syndrome) == arm::Trap::Hypercall(0) => {
      anyhow::ensure!(hv::get(&cpu, 0)? == 42, "Guest arithmetic failed");
      anyhow::ensure!(hv::get(&cpu, 2)? == 42, "Guest device readback failed");
      anyhow::ensure!(
        hv::get(&cpu, 31)? == 0x40000014,
        "Hypercall return PC is incorrect"
      );
    }
    exit => anyhow::bail!("Unexpected guest exit: {exit:?}"),
  }
  println!("Native ARM64 guest execution and device read/write passed");
  hv::gic::signal(&gic, 33, true)?;
  anyhow::ensure!(
    hv::gic::read(&gic, 0x204)? & 2 != 0,
    "Peripheral interrupt did not become pending"
  );
  hv::gic::signal(&gic, 33, false)?;
  println!("Native interrupt controller and peripheral signal passed");
  let kind = hv::gic::read_msi(&gic)?;
  anyhow::ensure!(
    (kind >> 16) & 0x3ff == 64 && kind & 0x3ff == 32,
    "Native MSI frame reports a different interrupt range"
  );
  anyhow::ensure!(
    hv::gic::signal(&gic, 64, true).is_err(),
    "MSI interrupt accepted through the wired signal path"
  );
  anyhow::ensure!(
    hv::gic::message(&gic, 0x30000044, 64).is_err()
      && hv::gic::message(&gic, 0x30000040, 96).is_err(),
    "Invalid MSI message target was accepted"
  );
  hv::gic::message(&gic, 0x30000040, 64)?;
  hv::gic::message(&gic, 0x30000040, 95)?;
  anyhow::ensure!(
    hv::gic::read(&gic, 0x208)? & 0x80000001 == 0x80000001,
    "Message interrupts did not become pending"
  );
  println!("Native MSI frame, bounded targets and message delivery passed");
  hv::write(&mut memory, 0, &0x14000000u32.to_le_bytes())?;
  hv::enter(&mut cpu, 0x40000000)?;
  let mut canceled = false;
  let result = hv::bounded(&mut cpu, std::time::Duration::from_millis(20), |cpu| {
    canceled = matches!(hv::run(cpu)?, hv::Exit::Canceled);
    Ok(())
  });
  anyhow::ensure!(
    canceled && result.is_err(),
    "Native CPU cancellation failed"
  );
  println!("Native CPU deadline cancellation passed");
  Ok(())
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("The native hypervisor requires Apple silicon macOS")
}
