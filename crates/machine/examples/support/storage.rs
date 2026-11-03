use anyhow::{ensure, Context};
use machine::{
  devices::virtio::{block, pci},
  dma::Memory,
  hypervisor::state::group,
};

const BASE: u64 = machine::platform::RAM + 0x50000;

pub fn device() -> anyhow::Result<pci::Device> {
  let file = tempfile::tempfile()?;
  file.set_len(64 * 1024 * 1024)?;
  fs2::FileExt::try_lock_exclusive(&file)?;
  let mut disk = block::attach(file, false, [b'd'; 20])?;
  block::writeback(&mut disk, true)?;
  pci::create(disk)
}

fn configure(device: &mut pci::Device, memory: &mut impl Memory) -> anyhow::Result<()> {
  pci::config_write(device, memory, 4, 2, 6)?;
  pci::config_write(device, memory, 0x10, 4, 0x21000004)?;
  for (offset, width, value) in [(20, 1, 3), (8, 4, 1), (12, 4, 1), (20, 1, 11), (24, 2, 8)] {
    pci::write(device, memory, offset, width, value)?;
  }
  for (offset, address) in [(32, BASE), (40, BASE + 0x100), (48, BASE + 0x200)] {
    pci::write(device, memory, offset, 4, address as u32)?;
  }
  pci::write(device, memory, 28, 2, 1)?;
  pci::write(device, memory, 20, 1, 15)?;
  Ok(())
}

pub fn prepare(device: &mut pci::Device, memory: &mut impl Memory) -> anyhow::Result<()> {
  configure(device, memory)?;
  transfer(device, memory, 1, true, b'a')
}

pub fn fresh(
  device: &mut pci::Device,
  memory: &mut impl Memory,
  expected: u8,
) -> anyhow::Result<()> {
  configure(device, memory)?;
  transfer(device, memory, 1, false, expected)?;
  transfer(device, memory, 2, true, b'e')?;
  transfer(device, memory, 3, false, b'e')
}

pub fn change(device: &mut pci::Device, memory: &mut impl Memory) -> anyhow::Result<()> {
  transfer(device, memory, 2, true, b'b')?;
  transfer(device, memory, 3, false, b'b')
}

fn transfer(
  device: &mut pci::Device,
  memory: &mut impl Memory,
  sequence: u16,
  write: bool,
  expected: u8,
) -> anyhow::Result<()> {
  let mut header = [0; 16];
  header[..4].copy_from_slice(&u32::from(write).to_le_bytes());
  memory.write(BASE + 0x1000, &header)?;
  memory.write(BASE + 0x2000, &[if write { expected } else { 0 }; 512])?;
  memory.write(BASE + 0x3000, &[255])?;
  for (index, (offset, length, flags, next)) in [
    (0x1000, 16u32, 1u16, 1u16),
    (0x2000, 512, if write { 1 } else { 3 }, 2),
    (0x3000, 1, 2, 0),
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
  pci::write(device, memory, pci::NOTIFY, 2, 0)?;
  let mut status = [255];
  memory.read(BASE + 0x3000, &mut status)?;
  ensure!(status == [0] && pci::completed(device) == u64::from(sequence));
  let mut bytes = [0; 512];
  memory.read(BASE + 0x2000, &mut bytes)?;
  ensure!(
    bytes == [expected; 512],
    "Recovered disk returned the wrong sector"
  );
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
  let saved = captured.primary.devices.transports[0]
    .as_ref()
    .context("Missing saved disk")?;
  {
    use std::{
      fs::File,
      os::unix::fs::{FileExt, PermissionsExt},
    };
    let root = tempfile::tempdir()?;
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))?;
    let prepared = pci::snapshot::prepare_disk(saved, &File::open(root.path())?, || Ok(()))?;
    let mut bytes = [0; 512];
    prepared.file().read_exact_at(&mut bytes, 0)?;
    ensure!(
      bytes == [b'a'; 512],
      "Persistent disk stage lost captured sectors"
    );
    prepared.file().write_all_at(&[b'p'; 512], 0)?;
    prepared.file().sync_all()?;
    drop(prepared);
    File::open(root.path().join("disk"))?.read_exact_at(&mut bytes, 0)?;
    ensure!(
      bytes == [b'p'; 512],
      "Persistent disk stage disappeared with its owner"
    );
  }
  for _ in 0..2 {
    let mut ram = Ram(vec![0; 65536]);
    captured.primary.memory.ram.read(0x50000, &mut ram.0)?;
    let mut restored = device()?;
    pci::snapshot::restore(saved, &mut restored, &ram)?;
    verify(&mut restored, &mut ram)?;
  }
  println!("Coordinated disk reconstruction preserves captured sectors after source writes and VM teardown; repeated recovery remains independent");
  Ok(())
}

pub fn verify(device: &mut pci::Device, memory: &mut impl Memory) -> anyhow::Result<()> {
  ensure!(pci::completed(device) == 1);
  transfer(device, memory, 2, false, b'a')?;
  transfer(device, memory, 3, true, b'c')?;
  transfer(device, memory, 4, false, b'c')
}

pub fn verify_recaptured(device: &mut pci::Device, memory: &mut impl Memory) -> anyhow::Result<()> {
  ensure!(pci::completed(device) == 4);
  transfer(device, memory, 5, false, b'c')?;
  transfer(device, memory, 6, true, b'd')?;
  transfer(device, memory, 7, false, b'd')
}
