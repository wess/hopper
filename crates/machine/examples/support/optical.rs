use anyhow::{ensure, Context};
use machine::{
  devices::virtio::{pci, scsi},
  dma::Memory,
  hypervisor::state::group,
};
use std::os::unix::fs::FileExt;

const BASE: u64 = machine::platform::RAM + 0x60000;

pub fn device() -> anyhow::Result<pci::Device> {
  let file = tempfile::NamedTempFile::new()?;
  file.as_file().write_all_at(&[0x5a; 4096], 0)?;
  pci::optical(scsi::open(file.path())?)
}

pub fn prepare(device: &mut pci::Device, memory: &mut impl Memory) -> anyhow::Result<()> {
  pci::config_write(device, memory, 4, 2, 6)?;
  pci::config_write(device, memory, 0x10, 4, 0x22000004)?;
  for (offset, width, value) in [
    (20, 1, 3),
    (8, 4, 1),
    (12, 4, 1),
    (20, 1, 11),
    (22, 2, 2),
    (24, 2, 8),
    (pci::SPECIFIC + 20, 4, 18),
    (pci::SPECIFIC + 24, 4, 16),
  ] {
    pci::write(device, memory, offset, width, value)?;
  }
  for (offset, address) in [(32, BASE), (40, BASE + 0x100), (48, BASE + 0x200)] {
    pci::write(device, memory, offset, 4, address as u32)?;
  }
  pci::write(device, memory, 28, 2, 1)?;
  pci::write(device, memory, 20, 1, 15)?;
  read(device, memory, 1)?;
  command(device, memory, 2, &[0x1b, 0, 0, 0, 2, 0], 0, 0)?;
  command(device, memory, 3, &[0], 0, 2)
}

pub fn change(device: &mut pci::Device, memory: &mut impl Memory) -> anyhow::Result<()> {
  command(device, memory, 4, &[0x1b, 0, 0, 0, 3, 0], 0, 0)?;
  read(device, memory, 5)
}

fn read(device: &mut pci::Device, memory: &mut impl Memory, sequence: u16) -> anyhow::Result<()> {
  command(
    device,
    memory,
    sequence,
    &[0x28, 0, 0, 0, 0, 1, 0, 0, 1, 0],
    2048,
    0,
  )?;
  let mut data = [0; 2048];
  memory.read(BASE + 0x3000, &mut data)?;
  ensure!(
    data == [0x5a; 2048],
    "Recovered optical media returned incorrect sectors"
  );
  Ok(())
}

fn command(
  device: &mut pci::Device,
  memory: &mut impl Memory,
  sequence: u16,
  cdb: &[u8],
  incoming: u32,
  status: u8,
) -> anyhow::Result<()> {
  let mut header = [0; 35];
  header[0] = 1;
  header[19..19 + cdb.len()].copy_from_slice(cdb);
  memory.write(BASE + 0x1000, &header)?;
  memory.write(BASE + 0x2000, &[255; 30])?;
  memory.write(BASE + 0x3000, &[0; 2048])?;
  for (index, (offset, length, flags, next)) in [
    (0x1000, 35u32, 1u16, 1u16),
    (0x2000, 30, 3, 2),
    (0x3000, incoming, 2, 0),
  ]
  .into_iter()
  .enumerate()
  {
    let mut descriptor = [0; 16];
    descriptor[..8].copy_from_slice(&(BASE + offset).to_le_bytes());
    descriptor[8..12].copy_from_slice(&length.to_le_bytes());
    descriptor[12..14].copy_from_slice(&flags.to_le_bytes());
    descriptor[14..].copy_from_slice(&next.to_le_bytes());
    memory.write(BASE + index as u64 * 16, &descriptor)?;
  }
  memory.write(
    BASE + 0x104 + u64::from((sequence - 1) % 8) * 2,
    &0u16.to_le_bytes(),
  )?;
  memory.write(BASE + 0x102, &sequence.to_le_bytes())?;
  pci::write(device, memory, pci::NOTIFY + 8, 2, 2)?;
  let mut response = [255; 2];
  memory.read(BASE + 0x200a, &mut response)?;
  ensure!(response == [status, 0] && pci::completed(device) == u64::from(sequence));
  Ok(())
}

struct Ram(Vec<u8>);
impl Memory for Ram {
  fn contains(&self, address: u64, length: usize) -> bool {
    address
      .checked_sub(BASE)
      .and_then(|offset| offset.checked_add(length as u64))
      .is_some_and(|end| end <= self.0.len() as u64)
  }
  fn read(&self, address: u64, bytes: &mut [u8]) -> anyhow::Result<()> {
    ensure!(self.contains(address, bytes.len()));
    let offset = (address - BASE) as usize;
    bytes.copy_from_slice(&self.0[offset..offset + bytes.len()]);
    Ok(())
  }
  fn write(&mut self, address: u64, bytes: &[u8]) -> anyhow::Result<()> {
    ensure!(self.contains(address, bytes.len()));
    let offset = (address - BASE) as usize;
    self.0[offset..offset + bytes.len()].copy_from_slice(bytes);
    Ok(())
  }
}

pub fn restore(captured: &group::Checkpoint) -> anyhow::Result<()> {
  let saved = captured.primary.devices.transports[3]
    .as_ref()
    .context("Missing saved optical media")?;
  for _ in 0..2 {
    let mut ram = Ram(vec![0; 65536]);
    captured.primary.memory.ram.read(0x60000, &mut ram.0)?;
    let mut restored = device()?;
    pci::snapshot::restore(saved, &mut restored, &ram)?;
    verify(&mut restored, &mut ram)?;
  }
  println!("Coordinated optical reconstruction preserves ejection, pending sense, negotiation and sectors after source VM teardown");
  Ok(())
}

pub fn verify(device: &mut pci::Device, memory: &mut impl Memory) -> anyhow::Result<()> {
  ensure!(pci::completed(device) == 3);
  ensure!(pci::read(device, pci::SPECIFIC + 20, 4)? == 18);
  ensure!(pci::read(device, pci::SPECIFIC + 24, 4)? == 16);
  command(device, memory, 4, &[3, 0, 0, 0, 18, 0], 18, 0)?;
  let mut sense = [0; 18];
  memory.read(BASE + 0x3000, &mut sense)?;
  ensure!(
    (sense[2], sense[12]) == (2, 0x3a),
    "Pending optical sense was lost"
  );
  command(device, memory, 5, &[0], 0, 2)?;
  command(device, memory, 6, &[0x1b, 0, 0, 0, 3, 0], 0, 0)?;
  read(device, memory, 7)?;
  Ok(())
}
