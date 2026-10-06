#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn main() -> anyhow::Result<()> {
  use anyhow::{bail, ensure, Context};
  use machine::{
    arm,
    devices::{flash as nor, pci, serial},
    acpi, hypervisor as hv, platform, psci, smccc,
  };
  use std::collections::VecDeque;
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
  hv::protect(&flash, 5)?;
  let mut variable_data = vec![0xff; 0x4000000];
  if let Some(path) = std::env::args().nth(3) {
    let template = std::fs::read(path)?;
    ensure!(
      !template.is_empty() && template.len() <= variable_data.len(),
      "Variable template exceeds its flash bank"
    );
    variable_data[..template.len()].copy_from_slice(&template);
  }
  let mut variables = nor::create(variable_data, 0x40000)?;
  let mut nvram = hv::memory(&vm, 0x4000000, 0x4000000)?;
  hv::write(&mut nvram, 0, nor::bytes(&variables))?;
  hv::protect(&nvram, 1)?;
  let mut ram = hv::memory(&vm, platform::RAM, topology.memory as usize)?;
  hv::write(&mut ram, 0, &tree)?;
  let tables = acpi::bundle(&topology)?;
  hv::write(&mut ram, (acpi::BASE - platform::RAM) as usize, &tables)?;
  let mut cpu = hv::cpu(&vm)?;
  hv::affinity(&mut cpu, 0)?;
  hv::set(&mut cpu, 0, platform::RAM)?;
  hv::enter(&mut cpu, 0)?;
  let mut console = serial::Console::default();
  let mut output = Vec::with_capacity(64);
  let mut opened_menu = false;
  let mut selected_shell = false;
  let mut input = VecDeque::new();
  let mut updates = 0usize;
  let mut bus = pci::Bus::default();
  let mut pci_reads = 0usize;
  let mut listed_acpi = false;
  let mut seen_acpi = [false; 4];
  let mut checked_acpi = [false; 2];
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
              if listed_acpi {
                for (index, signature) in [b"FACP", b"APIC", b"GTDT", b"DSDT"].iter().enumerate() {
                  seen_acpi[index] |= output.ends_with(*signature);
                }
                checked_acpi[0] |= output.ends_with(b"\t0 Error(s)");
                checked_acpi[1] |= output.ends_with(b"\t0 Warning(s)");
              }
              if output.ends_with(b"Shell> ") {
                ensure!(updates > 0, "Firmware did not update its variable flash");
                ensure!(input.is_empty(), "Firmware left diagnostic input queued");
                if listed_acpi {
                  ensure!(seen_acpi.iter().all(|seen| *seen), "UEFI did not expose all ACPI tables");
                  ensure!(checked_acpi.iter().all(|seen| *seen), "UEFI found ACPI errors or warnings");
                  ensure!(pci_reads > 0, "Firmware did not enumerate PCI configuration space");
                  eprintln!("\nFirmware verified ACPI tables, {pci_reads} PCI reads and {updates} variable flash updates");
                  return Ok(());
                }
                input.extend(b"acpiview\r");
                listed_acpi = true;
              }
              if !opened_menu && output.ends_with(b"Boot Manager Menu.") {
                input.push_back(b'\r');
                opened_menu = true;
              }
              if opened_menu && !selected_shell && output.ends_with(b"ESC to exit") {
                input.extend(b"\x1b[B\r");
                selected_shell = true;
              }
            }
          } else {
            if let Some(byte) = input.front().copied() {
              if serial::receive(&mut console, byte) { input.pop_front(); }
            }
            let value = serial::read(&mut console, offset);
            if access.register != 31 {
              hv::set(cpu, access.register.into(), value.into())?;
            }
          }
          let pc = hv::get(cpu, 31)?;
          hv::set(cpu, 31, pc.checked_add(4).context("Firmware PC overflow")?)?;
        }
        arm::Trap::DataAbort(Some(access)) if (platform::ECAM..platform::ECAM + pci::ECAM_SIZE).contains(&physical_address) => {
          let offset = physical_address - platform::ECAM;
          if access.write {
            let value = if access.register == 31 { 0 } else { hv::get(cpu, access.register.into())? };
            bus.write(offset, access.bytes.into(), value as u32)?;
          } else {
            let value = bus.read(offset, access.bytes.into())?;
            pci_reads += 1;
            if access.register != 31 { hv::set(cpu, access.register.into(), value as u64)?; }
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
              updates += 1;
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
        bail!("Unhandled firmware exit: {exit:?}");
      },
    }
    }
    bail!("Firmware diagnostic exhausted its exit budget")
  })?;
  let mut retained = vec![0; acpi::SIZE];
  hv::read(&ram, (acpi::BASE - platform::RAM) as usize, &mut retained)?;
  ensure!(retained == tables, "Firmware overwrote its reserved ACPI handoff");
  Ok(())
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("The native firmware probe requires Apple silicon macOS")
}
