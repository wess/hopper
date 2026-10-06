#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn main() -> anyhow::Result<()> {
  use anyhow::{bail, ensure, Context};
  use machine::{
    arm,
    devices::{flash as nor, serial},
    hypervisor as hv, platform, psci, smccc,
  };
  use std::io::Write;

  let path = std::env::args()
    .nth(1)
    .context("Provide an ARM64 EDK2 firmware image")?;
  let firmware = std::fs::read(path)?;
  ensure!(
    !firmware.is_empty() && firmware.len() <= 0x4000000,
    "Firmware exceeds its flash bank"
  );
  let vm = hv::create()?;
  let gic = hv::gic::create(&vm, platform::DISTRIBUTOR, platform::REDISTRIBUTOR)?;
  let topology = platform::Topology {
    memory: 0x10000000,
    cpus: 1,
    distributor_size: gic.distributor_size as u64,
    redistributor_size: gic.redistributor_size as u64,
  };
  let tree = platform::tree(&topology)?;
  if let Some(path) = std::env::args().nth(2) {
    std::fs::write(path, &tree)?;
  }
  let mut flash = hv::memory(&vm, 0, 0x4000000)?;
  hv::write(&mut flash, 0, &firmware)?;
  let mut variables = nor::create(vec![0xff; 0x4000000], 0x40000)?;
  let mut nvram = hv::memory(&vm, 0x4000000, 0x4000000)?;
  hv::write(&mut nvram, 0, nor::bytes(&variables))?;
  hv::protect(&nvram, 1)?;
  let mut ram = hv::memory(&vm, platform::RAM, topology.memory as usize)?;
  hv::write(&mut ram, 0, &tree)?;
  let mut cpu = hv::cpu(&vm)?;
  hv::affinity(&mut cpu, 0)?;
  hv::set(&mut cpu, 0, platform::RAM)?;
  hv::enter(&mut cpu, 0)?;
  let mut console = serial::Console::default();
  let mut output = Vec::with_capacity(64);
  let start = std::time::Instant::now();
  hv::bounded(&mut cpu, std::time::Duration::from_secs(30), |cpu| {
    for _ in 0..1000000 {
      ensure!(
        start.elapsed().as_secs() < 30,
        "Firmware diagnostic reached its deadline"
      );
      match hv::run(cpu)? {
      hv::Exit::Exception { syndrome, physical_address, .. } => match arm::decode(syndrome) {
        arm::Trap::DataAbort(Some(access)) if (platform::UART..platform::UART + 0x1000).contains(&physical_address) => {
          let offset = physical_address - platform::UART;
          ensure!(access.bytes <= 4, "Unsupported serial access width");
          if access.write {
            let value = if access.register == 31 { 0 } else { hv::get(cpu, access.register.into())? };
            if let Some(byte) = serial::write(&mut console, offset, value as u32) {
              std::io::stdout().write_all(&[byte])?;
              std::io::stdout().flush()?;
              if output.len() == 64 { output.remove(0); }
              output.push(byte);
              if output.ends_with(b"Shell> ") { return Ok(()); }
            }
          } else {
            let value = serial::read(&mut console, offset);
            if access.register != 31 {
              hv::set(cpu, access.register.into(), value.into())?;
            }
          }
          let pc = hv::get(cpu, 31)?;
          hv::set(cpu, 31, pc.checked_add(4).context("Firmware PC overflow")?)?;
        }
        arm::Trap::DataAbort(Some(access)) if (0x4000000..0x8000000).contains(&physical_address) => {
          let offset = (physical_address - 0x4000000) as usize;
          if access.write {
            ensure!(access.bytes == 4, "NOR command requires a 32-bit access");
            let value = if access.register == 31 { 0 } else { hv::get(cpu, access.register.into())? };
            if let Some(changed) = nor::write(&mut variables, offset, value as u32)? {
              hv::write(&mut nvram, changed.start, &nor::bytes(&variables)[changed])?;
            }
            hv::protect(&nvram, if nor::array(&variables) { 1 } else { 0 })?;
          } else {
            let value = nor::read(&variables, offset, access.bytes)?;
            if access.register != 31 { hv::set(cpu, access.register.into(), value)?; }
          }
          let pc = hv::get(cpu, 31)?;
          hv::set(cpu, 31, pc.checked_add(4).context("Firmware PC overflow")?)?;
        }
        arm::Trap::Hypercall(0) => {
          let command = hv::get(cpu, 0)? as u32;
          let argument = hv::get(cpu, 1)?;
          let level = hv::get(cpu, 2)? as u32;
          let mut entropy = |bytes: &mut [u8]| -> anyhow::Result<()> {
            if unsafe { libc::getentropy(bytes.as_mut_ptr().cast(), bytes.len()) } != 0 {
              return Err(std::io::Error::last_os_error()).context("Read system entropy");
            }
            Ok(())
          };
          if let Some(reply) = smccc::call(command, argument, &mut entropy) {
            for (register, value) in reply.into_iter().enumerate() {
              hv::set(cpu, register as u32, value)?;
            }
            continue;
          }
          match psci::call(command, argument, level) {
            psci::Reply::Value(value) => hv::set(cpu, 0, value as u64)?,
            reply => bail!("Firmware requested {reply:?}"),
          }
        }
        trap => bail!("Firmware stopped at PC 0x{:x}, address 0x{physical_address:x}, syndrome 0x{syndrome:x}: {trap:?}", hv::get(cpu, 31)?),
      },
      exit => {
        let pc = hv::get(cpu, 31)?;
        eprintln!("Firmware exit {exit:?} at PC 0x{pc:x}");
        for register in 0..8 {
          eprintln!("x{register}: 0x{:x}", hv::get(cpu, register)?);
        }
        if let Some(offset) = pc.checked_sub(platform::RAM) {
          let mut code = [0u8; 32];
          hv::read(&ram, offset as usize, &mut code)?;
          std::fs::write("/tmp/hoppernativepc.bin", code)?;
        }
        bail!("Unhandled firmware exit: {exit:?}");
      },
    }
    }
    bail!("Firmware diagnostic exhausted its exit budget")
  })
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("The native firmware probe requires Apple silicon macOS")
}
