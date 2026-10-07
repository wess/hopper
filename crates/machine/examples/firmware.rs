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
#[path = "support/disk.rs"]
mod target;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "support/queue.rs"]
mod queue;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn main() -> anyhow::Result<()> {
  use anyhow::{bail, ensure, Context};
  use machine::{
    arm,
    devices::{flash as nor, pci, serial, virtio::{block, console as channel, gpu, input as vinput, pci as vpci, scsi}},
    acpi, hypervisor as hv, platform, psci, smccc,
  };
  use hv::{registers as regs, secondary};
  use std::collections::VecDeque;
  use std::sync::{atomic::{AtomicBool, Ordering}, mpsc, Arc, Mutex};
  use std::io::Write;

  let path = std::env::args()
    .nth(1)
    .context("Provide an ARM64 EDK2 firmware image")?;
  let arguments: Vec<_> = std::env::args().collect();
  let boot_media = arguments.iter().any(|argument| argument == "--boot-media");
  let optical_boot = arguments.iter().any(|argument| argument == "--optical-boot");
  let serial_check = arguments.iter().any(|argument| argument == "--check-serial");
  ensure!(!serial_check || boot_media, "Serial check requires guest boot mode");
  ensure!(!optical_boot || boot_media, "Optical boot requires guest boot mode");
  let guest_input = arguments.iter().any(|argument| argument == "--check-guest-input");
  ensure!(!guest_input || boot_media, "Guest input check requires boot-only driver media");
  let target_path = arguments.iter().position(|argument| argument == "--check-guest-disk")
    .map(|index| arguments.get(index + 1).context("Provide a new disposable disk path"))
    .transpose()?;
  ensure!(target_path.is_none() || (boot_media && !guest_input),
    "Guest disk check requires boot-only driver media and its own input check");
  let optical_path = arguments.iter().position(|argument| argument == "--optical")
    .map(|index| arguments.get(index + 1).context("Provide an optical media image"))
    .transpose()?;
  ensure!(optical_path.is_none() || boot_media, "Optical media requires guest boot mode");
  let optical_check = arguments.iter().any(|argument| argument == "--check-optical");
  ensure!(!optical_check || (optical_path.is_some() && !guest_input && target_path.is_none()),
    "Optical check requires optical media and its own input check");
  let driver_check = arguments.iter().any(|argument| argument == "--check-driver");
  ensure!(!driver_check || (optical_path.is_some() && !guest_input && !optical_check && target_path.is_none()),
    "Driver check requires optical media and its own input check");
  let cpus = arguments.iter().position(|argument| argument == "--cpus")
    .map(|index| -> anyhow::Result<u32> { Ok(arguments.get(index + 1).context("Provide a CPU count")?.parse()?) })
    .transpose()?.unwrap_or(1);
  ensure!((1..=2).contains(&cpus), "Firmware diagnostic supports one or two CPUs");
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
    cpus,
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
  let mut debug: Vec<_> = (0..cpus).map(|_| machine::debug::State::default()).collect();
  let mut output = Vec::with_capacity(64);
  let mut opened_menu = false;
  let mut selected_shell = false;
  let mut input = VecDeque::new();
  let mut updates = 0usize;
  let mut bus = pci::Bus::default();
  let disk = std::env::args().nth(4).filter(|path| path != "-").map(|path| {
    if optical_boot { vpci::optical(scsi::open(std::path::Path::new(&path))?) }
    else { vpci::create(block::open(std::path::Path::new(&path), true, [0; 20])?) }
  }).transpose()?;
  let check_storage = arguments.iter().any(|argument| argument == "--check-storage");
  let check_input = arguments.iter().any(|argument| argument == "--check-input");
  let check_graphics = boot_media || check_input || arguments.iter().any(|argument| argument == "--check-graphics");
  let graphics = if check_graphics { Some(vpci::graphics(gpu::create(1024, 768)?)?) } else { None };

  ensure!(!check_storage || disk.is_some(), "Storage check requires a disk image");
  ensure!(!optical_boot || !check_storage, "Optical boot and block storage checks are separate");
  let keyboard = if check_input || boot_media { Some(vpci::controller(vinput::create(vinput::Kind::Keyboard))?) } else { None };
  let pointer = if check_input || boot_media { Some(vpci::controller(vinput::create(vinput::Kind::Tablet))?) } else { None };
  let target_disk = target_path.map(|path| target::create(std::path::Path::new(path))).transpose()?;
  let optical = optical_path.map(|path| vpci::optical(scsi::open(std::path::Path::new(path))?)).transpose()?;
  let serial_port = if serial_check { Some(vpci::serial(channel::create("org.hopper.setup")?)?) } else { None };
  let mut devices = [disk, graphics, keyboard, pointer, target_disk, optical, serial_port];
  let mut received_serial = Vec::new();
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
  let mut firmware_optical = None;
  let mut guest_pci = [[0usize; 2]; 7];
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
  let factory = hv::factory(&vm);
  let power = Arc::new(Mutex::new(psci::power::create(&(0..cpus as u64).collect::<Vec<_>>(),
    &[0..0x4000000, platform::RAM..platform::RAM + topology.memory])?));
  let boot = std::thread::scope(|scope| -> anyhow::Result<()> {
    let stop = Arc::new(AtomicBool::new(false));
    let _stopper = secondary::stopper(stop.clone());
    let (launch, launches) = mpsc::sync_channel(1);
    let (requests, pending) = mpsc::sync_channel(1);
    let worker = if cpus == 2 {
      let (ready, initialized) = mpsc::sync_channel(1);
      let config = secondary::Config {
        factory, index: 1, affinity: 1,
        timeout: std::time::Duration::from_secs(deadline),
        boot: launches, requests, wake: Some(hv::wake(&cpu)), ready, stop: stop.clone(), power: power.clone(),
      };
      let worker = scope.spawn(move || secondary::serve(config));
      initialized.recv_timeout(std::time::Duration::from_secs(5))?;
      Some(worker)
    } else { None };
    let boot = hv::paced(&mut cpu, std::time::Duration::from_secs(deadline), std::time::Duration::from_millis(20), |cpu| {
    for _ in 0..2000000 {
      if let Some(device) = &mut devices[6] {
        let bytes = vpci::receive_serial(device, &mut ram)?;
        ensure!(received_serial.len() + bytes.len() <= 4096, "Serial diagnostic output exceeded its bound");
        received_serial.extend(bytes);
        messages += interrupts::deliver(&gic, 6, device)?;
      }
      if (guest_input || target_path.is_some() || optical_check || driver_check) && !guest_sent && handed_off && start.elapsed().as_secs() >= 35
        && vpci::read(devices[2].as_mut().context("Missing keyboard")?, 20, 1)? == 15 {
        if driver_check {
          text_input.extend(b"pnputil /enum-devices /class scsiadapter\r");
        } else if optical_check {
          text_input.extend(b"pnputil /scan-devices\rdiskpart\rrescan\rlist volume\r");
        } else if target_path.is_some() {
          if optical_boot {
            text_input.extend(b"diskpart\rselect disk 0\rclean\rcreate partition primary\rlist partition\rlist volume\r");
          } else {
            text_input.extend(b"diskpart\rselect disk 1\rclean\rcreate partition primary\rlist partition\r");
          }
        } else {
          text_input.extend(b"echo hopper windows input verified\r");
        }
        read_input = true;
        guest_sent = true;
        eprintln!("Queued the Windows guest check command");
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
      let request = pending.try_recv().ok();
      let caller = usize::from(request.is_some());
      let exit = if let Some(request) = &request { request.exit } else { hv::run(cpu)? };
      if matches!(exit, hv::Exit::Canceled) {
        pulses += 1;
        if boot_media && start.elapsed().as_secs() >= 3 {
          fault::inspect(cpu, &ram, topology.memory)?;
        }
        continue;
      }
      let before = if let Some(request) = &request { request.registers.clone() } else { regs::capture(cpu)? };
      let mut registers = before.clone();
      let mut off = false;
      match exit {
      hv::Exit::Exception { syndrome, physical_address, .. } => match arm::decode(syndrome) {
        arm::Trap::DataAbort(Some(access)) if (platform::UART..platform::UART + 0x1000).contains(&physical_address) => {
          let offset = physical_address - platform::UART;
          ensure!(access.bytes <= 4, "Unsupported serial access width");
          if access.write {
            let value = if access.register == 31 { 0 } else { regs::read(&registers, access.register.into())? };
            if let Some(byte) = serial::write(&mut console, offset, value as u32) {
              std::io::stdout().write_all(&[byte])?;
              std::io::stdout().flush()?;
              if output.len() == 64 { output.remove(0); }
              output.push(byte);
              if boot_media && output.ends_with(b"VirtioInputExitBoot:") {
                if !handed_off { firmware_optical = devices[5].as_ref().map(vpci::completed); }
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
              regs::write(&mut registers, access.register.into(), value.into())?;
            }
          }
          let pc = regs::read(&registers, 31)?;
          regs::write(&mut registers, 31, pc.checked_add(4).context("Firmware PC overflow")?)?;
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
            let value = if access.register == 31 { 0 } else { regs::read(&registers, access.register.into())? };
            if let Some(device) = device {
              vpci::config_write(device, &mut ram, (offset & 0xfff) as usize, access.bytes.into(), value as u32)?;
            } else { bus.write(offset, access.bytes.into(), value as u32)?; }
          } else {
            let value = if let Some(device) = device {
              vpci::config_read(device, (offset & 0xfff) as usize, access.bytes.into())?
            } else { bus.read(offset, access.bytes.into())? };
            pci_reads += 1;
            if access.register != 31 { regs::write(&mut registers, access.register.into(), value as u64)?; }
          }
          for (index, device) in devices.iter_mut().enumerate() {
            if let Some(device) = device { messages += interrupts::deliver(&gic, index, device)?; }
          }
          let pc = regs::read(&registers, 31)?;
          regs::write(&mut registers, 31, pc.checked_add(4).context("Firmware PC overflow")?)?;
        }
        arm::Trap::DataAbort(Some(access)) if devices.iter().flatten().any(|device| device.pci.memory(physical_address).is_some()) => {
          let (index, device) = devices.iter_mut().enumerate().find_map(|(index, device)| {
            device.as_mut().filter(|device| device.pci.memory(physical_address).is_some()).map(|device| (index, device))
          }).context("Unmapped Virtio device")?;
          let (bar, offset) = device.pci.memory(physical_address).context("Unmapped Virtio BAR")?;
          if access.write {
            let value = if access.register == 31 { 0 } else { regs::read(&registers, access.register.into())? };
            vpci::memory_write(device, &mut ram, bar, offset, access.bytes.into(), value)?;
          } else {
            let value = vpci::memory_read(device, bar, offset, access.bytes.into())?;
            if access.register != 31 { regs::write(&mut registers, access.register.into(), value as u64)?; }
          }
          messages += interrupts::deliver(&gic, index, device)?;
          let pc = regs::read(&registers, 31)?;
          regs::write(&mut registers, 31, pc.checked_add(4).context("Firmware PC overflow")?)?;
        }
        arm::Trap::DataAbort(Some(access)) if (0x4000000..0x8000000).contains(&physical_address) => {
          let offset = (physical_address - 0x4000000) as usize;
          if access.write {
            ensure!(access.bytes == 4, "NOR command requires a 32-bit access");
            let value = if access.register == 31 { 0 } else { regs::read(&registers, access.register.into())? };
            if let Some(changed) = nor::write(&mut variables, offset, value as u32)? {
              hv::write(&mut nvram, changed.start, &nor::bytes(&variables)[changed])?;
              updates += 1;
            }
            hv::protect(&nvram, if nor::array(&variables) { 1 } else { 0 })?;
          } else {
            let value = nor::read(&variables, offset, access.bytes)?;
            if access.register != 31 { regs::write(&mut registers, access.register.into(), value)?; }
          }
          let pc = regs::read(&registers, 31)?;
          regs::write(&mut registers, 31, pc.checked_add(4).context("Firmware PC overflow")?)?;
        }
        arm::Trap::SystemRegister(access) => {
          let value = if access.read || access.register == 31 { 0 } else { regs::read(&registers, access.register.into())? };
          let result = machine::debug::access(&mut debug[caller], access, value)
            .with_context(|| format!("Unsupported guest system register 0x{:x}", access.encoding))?;
          if access.read && access.register != 31 { regs::write(&mut registers, access.register.into(), result)?; }
          let pc = regs::read(&registers, 31)?;
          regs::write(&mut registers, 31, pc.checked_add(4).context("Firmware PC overflow")?)?;
        }
        arm::Trap::Hypercall(0) => {
          let command = regs::read(&registers, 0)? as u32;
          let argument = regs::read(&registers, 1)?;
          let level = regs::read(&registers, 2)?;
          let mut entropy = |bytes: &mut [u8]| -> anyhow::Result<()> {
            if unsafe { libc::getentropy(bytes.as_mut_ptr().cast(), bytes.len()) } != 0 {
              return Err(std::io::Error::last_os_error()).context("Read system entropy");
            }
            Ok(())
          };
          if let Some(reply) = smccc::call(command, argument, &mut entropy) {
            for (register, value) in reply.into_iter().enumerate() {
              regs::write(&mut registers, register as u32, value)?;
            }
          } else {
            let context = regs::read(&registers, 3)?;
            let mut power = power.lock().map_err(|_| anyhow::anyhow!("CPU power lock poisoned"))?;
            let reply = if cpus == 1 { psci::call(command, argument, level as u32) }
              else { psci::power::call(&mut power, caller, command, [argument, level, context]) };
            match reply {
              psci::Reply::Value(value) => regs::write(&mut registers, 0, value as u64)?,
              psci::Reply::CpuOn { target, entry, context } => {
                ensure!(target == 1, "Unknown secondary CPU owner");
                debug[target] = machine::debug::State::default();
                let result = launch.try_send(secondary::Boot { entry, context });
                if result.is_err() { psci::power::failed(&mut power, target)?; }
                regs::write(&mut registers, 0, if result.is_ok() { 0 } else { (-6i64) as u64 })?;
                eprintln!("CPU_ON target {target}, entry 0x{entry:x}, accepted {}", result.is_ok());
              }
              psci::Reply::CpuOff if caller == 1 => off = true,
              reply => bail!("Firmware CPU {caller} requested {reply:?}"),
            }
          }
        }
        trap => bail!("Firmware stopped at PC 0x{:x}, address 0x{physical_address:x}, syndrome 0x{syndrome:x}: {trap:?}", regs::read(&registers, 31)?),
      },
      exit => {
        let pc = regs::read(&registers, 31)?;
        eprintln!("Firmware exit {exit:?} at PC 0x{pc:x}");
        bail!("Unhandled firmware exit: {exit:?}");
      },
    }
      if let Some(request) = request {
        request.reply.send(secondary::Response { registers, off })?;
      } else {
        regs::apply(cpu, &before, &registers)?;
      }
    }
    bail!("Firmware diagnostic exhausted its exit budget")
    });
    stop.store(true, Ordering::Release);
    if let Some(worker) = worker {
      let stats = worker.join().map_err(|_| anyhow::anyhow!("Secondary CPU owner panicked"))??;
      eprintln!("Secondary CPU runtime: {stats:?}");
    }
    boot
  });
  let boot = boot.with_context(|| {
    devices[0].as_ref().map(|disk| format!("Storage completed {} requests; fault: {:?}", vpci::completed(disk), vpci::fault(disk)))
      .unwrap_or_else(|| "Boot native firmware".into())
  });
  eprintln!("Firmware serviced {pulses} host polling exits");
  eprintln!("Delivered {messages} native MSI messages");
  if serial_check {
    eprintln!("Received {} guest serial bytes; marker verified: {}", received_serial.len(),
      received_serial == b"hopper native setup channel\r\n");
  }
  if boot_media && boot.is_err() {
    fault::report(&cpu)?;
  }
  if boot_media {
    if optical_boot {
      if let Some(device) = &mut devices[0] {
        eprintln!("Boot optical completed {} SCSI requests; fault: {:?}", vpci::completed(device), vpci::fault(device));
        queue::inspect(device, &mut ram)?;
      }
    }
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
    for device in devices[2..4].iter().flatten() {
      eprintln!("Installer completed {} input events; fault: {:?}", vpci::completed(device), vpci::fault(device));
    }
    if let Some(device) = &devices[4] {
      eprintln!("Target completed {} storage requests; fault: {:?}", vpci::completed(device), vpci::fault(device));
    }
    if let Some(device) = &mut devices[5] {
      eprintln!("Optical completed {} SCSI requests; fault: {:?}", vpci::completed(device), vpci::fault(device));
      eprintln!("Optical requests before firmware exit callback: {firmware_optical:?}");
      eprintln!("Optical command counts and check conditions: {:?}", vpci::optical_stats(device));
      queue::inspect(device, &mut ram)?;
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
    for device in devices[2..4].iter().flatten() {
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
