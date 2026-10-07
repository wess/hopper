#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn main() -> anyhow::Result<()> {
  use anyhow::{bail, ensure, Context};
  use machine::{
    arm,
    devices::{flash as nor, pci, serial, virtio::{block, gpu, pci as vpci}},
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
  let mut disk = std::env::args().nth(4).filter(|path| path != "-").map(|path| {
    vpci::create(block::open(std::path::Path::new(&path), true, [0; 20])?)
  }).transpose()?;
  let arguments: Vec<_> = std::env::args().collect();
  let check_storage = arguments.iter().any(|argument| argument == "--check-storage");
  let check_graphics = arguments.iter().any(|argument| argument == "--check-graphics");
  let mut graphics = if check_graphics { Some(vpci::graphics(gpu::create(1024, 768)?)?) } else { None };

  ensure!(!check_storage || disk.is_some(), "Storage check requires a disk image");
  let mut read_storage = false;
  let mut checked_storage = false;
  let mut pci_reads = 0usize;
  let mut listed_acpi = false;
  let mut seen_acpi = [false; 5];
  let mut checked_acpi = [false; 2];
  let start = std::time::Instant::now();
  let boot = hv::bounded(&mut cpu, std::time::Duration::from_secs(30), |cpu| {
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
                for (index, signature) in [b"FACP", b"APIC", b"GTDT", b"DSDT", b"MCFG"].iter().enumerate() {
                  seen_acpi[index] |= output.ends_with(*signature);
                }
                checked_acpi[0] |= output.ends_with(b"\t0 Error(s)");
                checked_acpi[1] |= output.ends_with(b"\t0 Warning(s)");
              }
              if read_storage { checked_storage |= output.ends_with(b"Hopper native storage verified"); }
              if output.ends_with(b"Shell> ") {
                ensure!(updates > 0, "Firmware did not update its variable flash");
                ensure!(input.is_empty(), "Firmware left diagnostic input queued");
                if listed_acpi {
                  ensure!(seen_acpi.iter().all(|seen| *seen), "UEFI did not expose all ACPI tables");
                  ensure!(checked_acpi.iter().all(|seen| *seen), "UEFI found ACPI errors or warnings");
                  ensure!(pci_reads > 0, "Firmware did not enumerate PCI configuration space");
                  if let Some(disk) = &disk {
                    ensure!(vpci::fault(disk).is_none(), "Virtio storage fault: {:?}", vpci::fault(disk));
                    ensure!(vpci::completed(disk) > 0, "UEFI did not read the disk");
                    if check_storage && !read_storage {
                      input.extend(b"type fs0:\\hopper.txt\r");
                      read_storage = true;
                    } else {
                      ensure!(!check_storage || checked_storage, "UEFI did not read the storage check file");
                      eprintln!("\nFirmware completed {} Virtio disk requests", vpci::completed(disk));
                      eprintln!("Firmware verified ACPI tables, {pci_reads} PCI reads and {updates} variable flash updates");
                      return Ok(());
                    }
                  } else {
                    eprintln!("\nFirmware verified ACPI tables, {pci_reads} PCI reads and {updates} variable flash updates");
                    return Ok(());
                  }
                } else {
                  input.extend(b"acpiview\r");
                  listed_acpi = true;
                }
              }
              if !opened_menu && output.ends_with(b"Boot Manager Menu.") {
                input.push_back(b'\r');
                opened_menu = true;
              }
              if opened_menu && !selected_shell && output.ends_with(b"ESC to exit") {
                if disk.is_some() { input.extend(b"\x1b[B\x1b[B\r"); }
                else { input.extend(b"\x1b[B\r"); }
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
            let device = if offset < 4096 { disk.as_mut() }
              else if (0x8000..0x9000).contains(&offset) { graphics.as_mut() } else { None };
            if let Some(disk) = device {
              vpci::config_write(disk, &mut ram, (offset & 0xfff) as usize, access.bytes.into(), value as u32)?;
            } else { bus.write(offset, access.bytes.into(), value as u32)?; }
          } else {
            let device = if offset < 4096 { disk.as_mut() }
              else if (0x8000..0x9000).contains(&offset) { graphics.as_mut() } else { None };
            let value = if let Some(disk) = device {
              vpci::config_read(disk, (offset & 0xfff) as usize, access.bytes.into())?
            } else { bus.read(offset, access.bytes.into())? };
            pci_reads += 1;
            if access.register != 31 { hv::set(cpu, access.register.into(), value as u64)?; }
          }
          if let Some(disk) = &disk { hv::gic::signal(&gic, pci::interrupt(0, 1)?, vpci::interrupt(disk))?; }
          if let Some(graphics) = &graphics { hv::gic::signal(&gic, pci::interrupt(1, 1)?, vpci::interrupt(graphics))?; }
          let pc = hv::get(cpu, 31)?;
          hv::set(cpu, 31, pc.checked_add(4).context("Firmware PC overflow")?)?;
        }
        arm::Trap::DataAbort(Some(access)) if disk.as_ref().is_some_and(|disk| disk.pci.memory(physical_address).is_some())
          || graphics.as_ref().is_some_and(|device| device.pci.memory(physical_address).is_some()) => {
          let (disk, index) = if disk.as_ref().is_some_and(|disk| disk.pci.memory(physical_address).is_some()) {
            (disk.as_mut().context("Missing Virtio disk")?, 0)
          } else { (graphics.as_mut().context("Missing Virtio GPU")?, 1) };
          let (_, offset) = disk.pci.memory(physical_address).context("Unmapped Virtio BAR")?;
          if access.write {
            let value = if access.register == 31 { 0 } else { hv::get(cpu, access.register.into())? };
            vpci::write(disk, &mut ram, offset, access.bytes.into(), value as u32)?;
          } else {
            let value = vpci::read(disk, offset, access.bytes.into())?;
            if access.register != 31 { hv::set(cpu, access.register.into(), value as u64)?; }
          }
          hv::gic::signal(&gic, pci::interrupt(index, 1)?, vpci::interrupt(disk))?;
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
  });
  boot.with_context(|| {
    disk.as_ref().map(|disk| format!("Storage completed {} requests; fault: {:?}", vpci::completed(disk), vpci::fault(disk)))
      .unwrap_or_else(|| "Boot native firmware".into())
  })?;
  if let Some(graphics) = &graphics {
    ensure!(vpci::fault(graphics).is_none(), "Virtio GPU fault: {:?}", vpci::fault(graphics));
    let frame = vpci::display(graphics).and_then(gpu::frame).context("Firmware produced no GPU frame")?;
    ensure!(vpci::completed(graphics) > 0 && frame.rgba.as_chunks::<4>().0.iter()
      .any(|pixel| pixel[..3] != [0, 0, 0]), "Firmware produced no visible graphics");
    eprintln!("Firmware rendered {}x{} frame in {} GPU requests", frame.width, frame.height, vpci::completed(graphics));
    if let Some(index) = arguments.iter().position(|argument| argument == "--frame") {
      let path = arguments.get(index + 1).context("Provide a frame output path")?;
      let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(path)?;
      write!(file, "P6\n{} {}\n255\n", frame.width, frame.height)?;
      for pixel in frame.rgba.as_chunks::<4>().0 { file.write_all(&pixel[..3])?; }
    }
  }
  let mut retained = vec![0; acpi::SIZE];
  hv::read(&ram, (acpi::BASE - platform::RAM) as usize, &mut retained)?;
  ensure!(retained == tables, "Firmware overwrote its reserved ACPI handoff");
  Ok(())
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("The native firmware probe requires Apple silicon macOS")
}
