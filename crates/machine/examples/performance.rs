#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn main() -> anyhow::Result<()> {
  use anyhow::{bail, ensure, Context};
  use machine::{arm, debug, hypervisor as hv};

  let vm = hv::create()?;
  let _gic = hv::gic::create(&vm, 0x08000000, 0x0a000000)?;
  let mut ram = hv::memory(&vm, 0x40000000, 0x4000)?;
  // enable the virtual cycle counter, run a loop, then unlock the guest OS debug lock.
  let code = [
    0xd5380503u32,
    0xd53b9c04,
    0xb2400084,
    0xd51b9c04,
    0xd2b00004,
    0xd51b9c24,
    0xd5033fdf,
    0xd53b9d05,
    0xd2820006,
    0xf10004c6,
    0x54ffffe1,
    0xd53b9d07,
    0xd5301189,
    0xd510109f,
    0xd530118a,
    0xd4000002,
  ];
  let bytes: Vec<_> = code.into_iter().flat_map(u32::to_le_bytes).collect();
  hv::write(&mut ram, 0, &bytes)?;
  let mut cpu = hv::cpu(&vm)?;
  hv::enter(&mut cpu, 0x40000000)?;
  let mut debug = debug::State::default();
  let mut locks = 0;
  hv::bounded(&mut cpu, std::time::Duration::from_secs(2), |cpu| {
    for _ in 0..8 {
      let hv::Exit::Exception { syndrome, .. } = hv::run(cpu)? else {
        bail!("Performance probe did not reach a guest instruction trap");
      };
      match arm::decode(syndrome) {
        arm::Trap::Hypercall(0) => return Ok(()),
        arm::Trap::SystemRegister(access) => {
          let value = if access.read || access.register == 31 {
            0
          } else {
            hv::get(cpu, access.register.into())?
          };
          let reply = debug::access(&mut debug, access, value)
            .with_context(|| format!("Unexpected system register trap 0x{:x}", access.encoding))?;
          if access.read && access.register != 31 {
            hv::set(cpu, access.register.into(), reply)?;
          }
          hv::set(cpu, 31, hv::get(cpu, 31)? + 4)?;
          locks += 1;
        }
        trap => bail!("Unexpected performance probe trap: {trap:?}"),
      }
    }
    bail!("Performance probe exhausted its exit budget")
  })?;
  ensure!(
    hv::get(&cpu, 3)? & 0xf00 == 0x100,
    "Guest PMU version differs from configuration"
  );
  ensure!(
    hv::get(&cpu, 7)? > hv::get(&cpu, 5)?,
    "Virtual cycle counter did not advance"
  );
  ensure!(
    locks == 3 && hv::get(&cpu, 9)? == 10 && hv::get(&cpu, 10)? == 8,
    "Guest OS lock did not transition from locked to unlocked"
  );
  println!("Native virtual performance counter and guest OS debug lock passed");
  Ok(())
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("The performance probe requires Apple silicon macOS")
}
