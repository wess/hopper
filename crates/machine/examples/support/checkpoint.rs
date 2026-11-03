use anyhow::{ensure, Context};
use machine::hypervisor::{self as hv, state::group};
use machine::{
  devices::virtio::{gpu, input, pci},
  dma::Memory,
};
use std::time::Duration;

pub fn verify(captured: &group::Checkpoint, expected: [u64; 2]) -> anyhow::Result<()> {
  let memory = &captured.primary.memory;
  ensure!(memory.ram.guest() == machine::platform::RAM && memory.ram.size() == 0x10000000);
  ensure!(memory.firmware.guest() == 0 && memory.firmware.size() == 0x4000000);
  ensure!(memory.nvram.guest() == 0x4000000 && memory.nvram.size() == 0x4000000);
  let mut bytes = [0; 16];
  memory.ram.read(0x10000, &mut bytes)?;
  ensure!(u64::from_le_bytes(bytes[..8].try_into()?) == expected[0]);
  ensure!(u64::from_le_bytes(bytes[8..].try_into()?) == expected[1]);
  memory.firmware.read(0, &mut bytes)?;
  ensure!(u32::from_le_bytes(bytes[..4].try_into()?) == 0x52800060);
  memory.nvram.read(0, &mut bytes)?;
  ensure!(bytes == [0xff; 16]);
  let mut marker = [0; 8];
  memory.ram.read(0x20000, &mut marker)?;
  ensure!(
    marker == 123u64.to_le_bytes(),
    "Request callback's final RAM update was lost"
  );
  let mut rtc = captured.primary.devices.rtc.clone();
  let host = machine::devices::rtc::now()?;
  let guest = machine::devices::rtc::read(&mut rtc, 0, host)?;
  ensure!(
    (3599..=3600).contains(&(i64::from(guest) - i64::from(host))),
    "Guest RTC adjustment was not preserved independently of host UTC"
  );
  let mut observed = [0; 4];
  memory.ram.read(0x10020, &mut observed)?;
  ensure!(
    u32::from_le_bytes(observed) != 0,
    "Guest did not execute real RTC reads"
  );
  ensure!(!captured.primary.interrupts.bytes().is_empty());
  ensure!(captured.primary.devices.transports[2].is_some());
  memory.ram.read(0x30202, &mut marker[..2])?;
  ensure!(&marker[..2] == 2u16.to_le_bytes());
  Ok(())
}

pub fn device() -> anyhow::Result<pci::Device> {
  pci::controller(input::create(input::Kind::Keyboard))
}

pub fn graphics() -> anyhow::Result<pci::Device> {
  pci::graphics(gpu::create(640, 480)?)
}

pub fn prepare_graphics(device: &mut pci::Device, memory: &mut impl Memory) -> anyhow::Result<()> {
  let base = machine::platform::RAM + 0x40000;
  memory.write(base, &[1, 2, 3, 0].repeat(4))?;
  for (offset, value) in [
    (8, base as u32),
    (12, 0),
    (16, 2),
    (20, 2),
    (24, 2),
    (28, 1),
  ] {
    pci::write(device, memory, gpu::linear::BASE + offset, 4, value)?;
  }
  pci::refresh(device, memory)?;
  Ok(())
}

pub fn change_graphics(device: &mut pci::Device, memory: &mut impl Memory) -> anyhow::Result<()> {
  memory.write(machine::platform::RAM + 0x40000, &[20; 16])?;
  pci::refresh(device, memory)?;
  ensure!(
    gpu::frame(pci::display(device).context("Missing display")?)
      .context("Missing frame")?
      .rgba
      == [20, 20, 20, 255].repeat(4)
  );
  Ok(())
}

pub fn prepare_transport(device: &mut pci::Device, memory: &mut impl Memory) -> anyhow::Result<()> {
  let base = machine::platform::RAM + 0x30000;
  pci::config_write(device, memory, 4, 2, 6)?;
  pci::config_write(device, memory, 0x10, 4, 0x20000004)?;
  for (offset, width, value) in [(20, 1, 3), (8, 4, 1), (12, 4, 1), (20, 1, 11), (24, 2, 8)] {
    pci::write(device, memory, offset, width, value)?;
  }
  for (offset, address) in [(32, base), (40, base + 0x100), (48, base + 0x200)] {
    pci::write(device, memory, offset, 4, address as u32)?;
  }
  pci::write(device, memory, 28, 2, 1)?;
  pci::write(device, memory, 20, 1, 15)?;
  for index in 0..8u16 {
    let mut descriptor = [0; 16];
    descriptor[..8].copy_from_slice(&(base + 0x1000 + u64::from(index) * 8).to_le_bytes());
    descriptor[8..12].copy_from_slice(&8u32.to_le_bytes());
    descriptor[12..14].copy_from_slice(&2u16.to_le_bytes());
    memory.write(base + u64::from(index) * 16, &descriptor)?;
    memory.write(base + 0x104 + u64::from(index) * 2, &index.to_le_bytes())?;
  }
  memory.write(base + 0x102, &2u16.to_le_bytes())?;
  let key = |code| input::Event {
    kind: 1,
    code,
    value: 1,
  };
  pci::send_input(device, memory, &[key(30), input::SYN])?;
  pci::send_input(device, memory, &[key(42), input::SYN])?;
  ensure!(pci::completed(device) == 2 && pci::interrupt(device));
  Ok(())
}

pub fn restore_device(captured: &group::Checkpoint) -> anyhow::Result<()> {
  struct Ram {
    base: u64,
    bytes: Vec<u8>,
  }
  impl Memory for Ram {
    fn contains(&self, address: u64, length: usize) -> bool {
      address
        .checked_sub(self.base)
        .and_then(|offset| offset.checked_add(length as u64))
        .is_some_and(|end| end <= self.bytes.len() as u64)
    }
    fn read(&self, address: u64, bytes: &mut [u8]) -> anyhow::Result<()> {
      ensure!(
        self.contains(address, bytes.len()),
        "Read outside reconstructed device RAM"
      );
      let offset = (address - self.base) as usize;
      bytes.copy_from_slice(&self.bytes[offset..offset + bytes.len()]);
      Ok(())
    }
    fn write(&mut self, address: u64, bytes: &[u8]) -> anyhow::Result<()> {
      ensure!(
        self.contains(address, bytes.len()),
        "Write outside reconstructed device RAM"
      );
      let offset = (address - self.base) as usize;
      self.bytes[offset..offset + bytes.len()].copy_from_slice(bytes);
      Ok(())
    }
  }
  let saved = captured.primary.devices.transports[2]
    .as_ref()
    .context("Missing saved keyboard")?;
  let mut ram = Ram {
    base: machine::platform::RAM + 0x30000,
    bytes: vec![0; 131072],
  };
  for (index, chunk) in ram.bytes.chunks_mut(65536).enumerate() {
    captured
      .primary
      .memory
      .ram
      .read(0x30000 + index * 65536, chunk)?;
  }
  let mut restored = device()?;
  pci::snapshot::restore(saved, &mut restored, &ram)?;
  ensure!(pci::completed(&restored) == 2 && pci::interrupt(&restored));
  ensure!(restored.pci.memory(0x20000000) == Some((0, 0)));
  ensure!(pci::read(&mut restored, pci::ISR, 1)? == 1 && !pci::interrupt(&restored));
  ram.write(ram.base + 0x102, &4u16.to_le_bytes())?;
  pci::write(&mut restored, &mut ram, pci::NOTIFY, 2, 0)?;
  ensure!(pci::completed(&restored) == 4);
  ensure!(&ram.bytes[0x1010..0x1018] == [1, 0, 42, 0, 1, 0, 0, 0]);
  pci::release_input(&mut restored, &mut ram)?;
  ram.write(ram.base + 0x102, &7u16.to_le_bytes())?;
  pci::write(&mut restored, &mut ram, pci::NOTIFY, 2, 0)?;
  ensure!(pci::completed(&restored) == 7);
  ensure!(&ram.bytes[0x1020..0x1028] == [1, 0, 30, 0, 0, 0, 0, 0]);
  ensure!(&ram.bytes[0x1028..0x1030] == [1, 0, 42, 0, 0, 0, 0, 0]);
  let mut original = [0; 2];
  captured.primary.memory.ram.read(0x30202, &mut original)?;
  ensure!(original == 2u16.to_le_bytes());
  let saved_graphics = captured.primary.devices.transports[1]
    .as_ref()
    .context("Missing saved graphics")?;
  let mut display = graphics()?;
  pci::snapshot::restore(saved_graphics, &mut display, &ram)?;
  ensure!(
    gpu::frame(pci::display(&display).context("Missing restored display")?)
      .context("Missing restored frame")?
      .rgba
      == [3, 2, 1, 255].repeat(4)
  );
  pci::refresh(&mut display, &ram)?;
  ensure!(
    gpu::frame(pci::display(&display).context("Missing restored display")?)
      .context("Missing restored frame")?
      .rgba
      == [3, 2, 1, 255].repeat(4)
  );
  println!("Coordinated graphics reconstruction preserves captured framebuffer after source pixels change and VM teardown");
  println!("Coordinated keyboard reconstruction preserves pending events/held-key release after source teardown and leaves captured RAM unchanged");
  Ok(())
}

pub fn restore_interrupts(captured: &group::Checkpoint) -> anyhow::Result<()> {
  let vm = hv::create()?;
  let gic = hv::gic::create_msi(
    &vm,
    machine::platform::DISTRIBUTOR,
    machine::platform::REDISTRIBUTOR,
    machine::platform::MSI,
    64,
    32,
  )?;
  let mut primary = hv::cpu(&vm)?;
  hv::affinity(&mut primary, 0)?;
  let factory = hv::factory(&vm);
  std::thread::scope(|scope| -> anyhow::Result<()> {
    let (ready, initialized) = std::sync::mpsc::sync_channel(1);
    let (release, held) = std::sync::mpsc::sync_channel(1);
    let secondary = scope.spawn(move || -> anyhow::Result<()> {
      let mut cpu = hv::create_cpu(factory)?;
      hv::affinity(&mut cpu, 1)?;
      ready.send(())?;
      held.recv_timeout(Duration::from_secs(5))?;
      Ok(())
    });
    let result = (|| {
      initialized
        .recv_timeout(Duration::from_secs(5))
        .context("Initialize restore CPU")?;
      hv::gic::state::restore(&gic, &captured.primary.interrupts)
    })();
    let _ = release.send(());
    secondary
      .join()
      .map_err(|_| anyhow::anyhow!("Restore CPU owner panicked"))??;
    result
  })?;
  Ok(())
}
