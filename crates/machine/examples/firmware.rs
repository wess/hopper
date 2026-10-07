#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "support/input.rs"]
mod keyboard;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "support/frame.rs"]
mod frame;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "support/fault.rs"]
mod fault;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "support/interrupt.rs"]
mod interrupts;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn main() -> anyhow::Result<()> {
  use anyhow::{bail, ensure, Context};
  use machine::{
    arm,
    devices::{flash as nor, pci, serial, virtio::{block, gpu, input as vinput, pci as vpci}},
    acpi, hypervisor as hv, platform, psci, smccc,
  };
  use std::collections::VecDeque;
  use std::io::Write;

  let path = std::env::args()
    .nth(1)
    .context("Provide an ARM64 EDK2 firmware image")?;
  let arguments: Vec<_> = std::env::args().collect();
  let boot_media = arguments.iter().any(|argument| argument == "--boot-media");
  let guest_input = arguments.iter().any(|argument| argument == "--check-guest-input");
  ensure!(!guest_input || boot_media, "Guest input check requires boot-only driver media");
  let firmware = std::fs::read(path)?;
  ensure!(
    !firmware.is_empty() && firmware.len() <= 0x4000000,
    "Firmware exceeds its flash bank"
  );
  let vm = hv::create()?;
  let gic = hv::gic::create_msi(&vm, platform::DISTRIBUTOR, platform::REDISTRIBUTOR,
    platform::MSI, 64, 32)?;
  let topology = platform::Topology {
    memory: if boot_media { 0x100000000 } else { 0x10000000 },
    cpus: 1,
    distributor_size: gic.distributor_size as u64,
    redistributor_size: gic.redistributor_size as u64,
    msi: gic.msi.as_ref().map(|msi| platform::Msi {
      size: msi.size as u64, first: msi.first, count: msi.count,
    }),
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
  let mut debug = machine::debug::State::default();
  let mut output = Vec::with_capacity(64);
  let mut opened_menu = false;
  let mut selected_shell = false;
  let mut input = VecDeque::new();
  let mut updates = 0usize;
  let mut bus = pci::Bus::default();
  let disk = std::env::args().nth(4).filter(|path| path != "-").map(|path| {
    vpci::create(block::open(std::path::Path::new(&path), true, [0; 20])?)
  }).transpose()?;
  let check_storage = arguments.iter().any(|argument| argument == "--check-storage");
  let check_input = arguments.iter().any(|argument| argument == "--check-input");
  let check_graphics = boot_media || check_input || arguments.iter().any(|argument| argument == "--check-graphics");
  let graphics = if check_graphics { Some(vpci::graphics(gpu::create(1024, 768)?)?) } else { None };

  ensure!(!check_storage || disk.is_some(), "Storage check requires a disk image");
  let keyboard = if check_input || boot_media { Some(vpci::controller(vinput::create(vinput::Kind::Keyboard))?) } else { None };
  let pointer = if check_input || boot_media { Some(vpci::controller(vinput::create(vinput::Kind::Tablet))?) } else { None };
  let mut devices = [disk, graphics, keyboard, pointer];
  ensure!(!boot_media || devices[0].is_some(), "Media boot requires an installer image");
  ensure!(!boot_media || (!check_input && !check_storage), "Media boot and shell checks are separate modes");
  let mut media_key = None;
  let mut text_input = VecDeque::new();
  let mut next_input = std::time::Instant::now();
  let mut read_input = false;
  let mut checked_input = false;
  let mut read_storage = false;
  let mut checked_storage = false;
  let mut pci_reads = 0usize;
  let mut handed_off = false;
  let mut guest_pci = [[0usize; 2]; 4];
  let mut guest_bus = [0usize; 2];
  let mut listed_acpi = false;
  let mut seen_acpi = [false; 5];
  let mut checked_acpi = [false; 2];
  let start = std::time::Instant::now();
  let deadline = if let Some(index) = arguments.iter().position(|argument| argument == "--seconds") {
    let seconds: u64 = arguments.get(index + 1).context("Provide a diagnostic duration")?.parse()?;
    ensure!((3..=300).contains(&seconds), "Diagnostic duration must be 3–300 seconds");
    seconds
  } else if check_input || boot_media { 60 } else { 30 };
  let mut pulses = 0;
  let mut messages = 0usize;
  let mut guest_sent = false;
  let boot = hv::paced(&mut cpu, std::time::Duration::from_secs(deadline), std::time::Duration::from_millis(20), |cpu| {
    for _ in 0..2000000 {
      if guest_input && !guest_sent && handed_off && start.elapsed().as_secs() >= 35 {
        ensure!(vpci::read(devices[2].as_mut().context("Missing keyboard")?, 20, 1)? == 15,
          "Guest keyboard driver is not ready");
        text_input.extend(b"echo hopper windows input verified\r");
        read_input = true;
        guest_sent = true;
        eprintln!("Queued the Windows keyboard check command");
      }
      if media_key.is_some_and(|time| time <= std::time::Instant::now()) {
        vpci::send_input(devices[2].as_mut().context("Missing keyboard")?, &mut ram, &keyboard::text(b"\r")?)?;
        messages += interrupts::deliver(&gic, 2, devices[2].as_mut().context("Missing keyboard")?)?;
        media_key = None;
        eprintln!("Firmware queued the installer Enter key");
      }
      if read_input && !text_input.is_empty() && next_input <= std::time::Instant::now() {
        let byte = text_input.pop_front().context("Missing diagnostic key")?;
        vpci::send_input(devices[2].as_mut().context("Missing keyboard")?, &mut ram, &keyboard::text(&[byte])?)?;
        messages += interrupts::deliver(&gic, 2, devices[2].as_mut().context("Missing keyboard")?)?;
        next_input = std::time::Instant::now() + std::time::Duration::from_millis(150);
      }
      ensure!(
        start.elapsed().as_secs() < deadline,
        "Firmware diagnostic reached its deadline"
      );
      match hv::run(cpu)? {
      hv::Exit::Canceled => {
        pulses += 1;
        if boot_media && start.elapsed().as_secs() >= 3 {
          fault::inspect(cpu, &ram, topology.memory)?;
        }
        continue;
      }
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
              if boot_media && output.ends_with(b"VirtioInputExitBoot:") {
                handed_off = true;
              }
              if boot_media && output.ends_with(b"cdboot.efi") {
                media_key = Some(std::time::Instant::now() + std::time::Duration::from_secs(1));
              }
              if listed_acpi {
                for (index, signature) in [b"FACP", b"APIC", b"GTDT", b"DSDT", b"MCFG"].iter().enumerate() {
                  seen_acpi[index] |= output.ends_with(*signature);
                }
                checked_acpi[0] |= output.ends_with(b"\t0 Error(s)");
                checked_acpi[1] |= output.ends_with(b"\t0 Warning(s)");
              }
              if read_input { checked_input |= output.ends_with(b"hopper native input verified"); }
              if read_storage { checked_storage |= output.ends_with(b"Hopper native storage verified"); }
              if output.ends_with(b"Shell> ") {
                ensure!(updates > 0, "Firmware did not update its variable flash");
                ensure!(input.is_empty(), "Firmware left diagnostic input queued");
                if listed_acpi {
                  ensure!(seen_acpi.iter().all(|seen| *seen), "UEFI did not expose all ACPI tables");
                  ensure!(checked_acpi.iter().all(|seen| *seen), "UEFI found ACPI errors or warnings");
                  ensure!(pci_reads > 0, "Firmware did not enumerate PCI configuration space");
                  if check_input && !read_input {
                    text_input.extend(b"echo hopper native input verified\r");
                    let events = [vinput::Event { kind: 3, code: 0, value: 32768 },
                      vinput::Event { kind: 3, code: 1, value: 32768 }, vinput::SYN];
                    vpci::send_input(devices[3].as_mut().context("Missing pointer")?, &mut ram, &events)?;
                    for (index, device) in devices.iter_mut().enumerate().skip(2) {
                      if let Some(device) = device { messages += interrupts::deliver(&gic, index, device)?; }
                    }
                    read_input = true;
                  } else if let Some(disk) = &devices[0] {
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
              if !boot_media && !opened_menu && output.ends_with(b"Boot Manager Menu.") {
                input.push_back(b'\r');
                opened_menu = true;
              }
              if opened_menu && !selected_shell && output.ends_with(b"ESC to exit") {
                if devices[0].is_some() { input.extend(b"\x1b[B\x1b[B\r"); }
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
          let index = (offset >> 15) as usize;
          if handed_off {
            let direction = usize::from(access.write);
            guest_bus[direction] += 1;
            if let Some(counts) = guest_pci.get_mut(index) {
              counts[direction] += 1;
            }
          }
          let device = devices.get_mut(index).and_then(Option::as_mut).filter(|_| offset & 0x7000 == 0);
          if access.write {
            let value = if access.register == 31 { 0 } else { hv::get(cpu, access.register.into())? };
            if let Some(device) = device {
              vpci::config_write(device, &mut ram, (offset & 0xfff) as usize, access.bytes.into(), value as u32)?;
            } else { bus.write(offset, access.bytes.into(), value as u32)?; }
          } else {
            let value = if let Some(device) = device {
              vpci::config_read(device, (offset & 0xfff) as usize, access.bytes.into())?
            } else { bus.read(offset, access.bytes.into())? };
            pci_reads += 1;
            if access.register != 31 { hv::set(cpu, access.register.into(), value as u64)?; }
          }
          for (index, device) in devices.iter_mut().enumerate() {
            if let Some(device) = device { messages += interrupts::deliver(&gic, index, device)?; }
          }
          let pc = hv::get(cpu, 31)?;
          hv::set(cpu, 31, pc.checked_add(4).context("Firmware PC overflow")?)?;
        }
        arm::Trap::DataAbort(Some(access)) if devices.iter().flatten().any(|device| device.pci.memory(physical_address).is_some()) => {
          let (index, device) = devices.iter_mut().enumerate().find_map(|(index, device)| {
            device.as_mut().filter(|device| device.pci.memory(physical_address).is_some()).map(|device| (index, device))
          }).context("Unmapped Virtio device")?;
          let (bar, offset) = device.pci.memory(physical_address).context("Unmapped Virtio BAR")?;
          if access.write {
            let value = if access.register == 31 { 0 } else { hv::get(cpu, access.register.into())? };
            vpci::memory_write(device, &mut ram, bar, offset, access.bytes.into(), value)?;
          } else {
            let value = vpci::memory_read(device, bar, offset, access.bytes.into())?;
            if access.register != 31 { hv::set(cpu, access.register.into(), value as u64)?; }
          }
          messages += interrupts::deliver(&gic, index, device)?;
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
        arm::Trap::SystemRegister(access) => {
          let value = if access.read || access.register == 31 { 0 } else { hv::get(cpu, access.register.into())? };
          let result = machine::debug::access(&mut debug, access, value)
            .with_context(|| format!("Unsupported guest system register 0x{:x}", access.encoding))?;
          if access.read && access.register != 31 { hv::set(cpu, access.register.into(), result)?; }
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
  let boot = boot.with_context(|| {
    devices[0].as_ref().map(|disk| format!("Storage completed {} requests; fault: {:?}", vpci::completed(disk), vpci::fault(disk)))
      .unwrap_or_else(|| "Boot native firmware".into())
  });
  eprintln!("Firmware serviced {pulses} host polling exits");
  eprintln!("Delivered {messages} native MSI messages");
  if boot_media && boot.is_err() {
    fault::report(&cpu)?;
  }
  if boot_media {
    eprintln!("Firmware exit callback observed: {handed_off}");
    eprintln!("Post-handoff PCI configuration: {} reads, {} writes",
      guest_bus[0], guest_bus[1]);
    for (index, device) in devices.iter_mut().enumerate() {
      if let Some(device) = device {
        eprintln!("PCI device {index}: {} reads, {} writes, command 0x{:04x}, BAR0 0x{:08x}, Virtio status 0x{:02x}",
          guest_pci[index][0], guest_pci[index][1], device.pci.read(4, 2)?,
          device.pci.read(0x10, 4)?, vpci::read(device, 20, 1)?);
      }
    }
    for device in devices[2..].iter().flatten() {
      eprintln!("Installer completed {} input events; fault: {:?}", vpci::completed(device), vpci::fault(device));
    }
  }
  if let Some(graphics) = &mut devices[1] {
    let capture = vpci::refresh(graphics, &ram).and_then(|_| frame::export(graphics, &arguments));
    if boot.is_ok() { capture?; }
    else if let Err(error) = capture { eprintln!("Frame capture failed: {error:#}"); }
  }
  boot?;
  if check_input {
    ensure!(read_input && checked_input, "Firmware did not execute the keyboard input check");
    for device in devices[2..].iter().flatten() {
      ensure!(vpci::fault(device).is_none() && vpci::completed(device) > 0,
        "Firmware input device failed: {:?}", vpci::fault(device));
      eprintln!("Firmware completed {} Virtio input events", vpci::completed(device));
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
