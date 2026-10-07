use machine::{devices::virtio::pci, dma::Memory};

pub fn inspect(device: &mut pci::Device, memory: &mut impl Memory) -> anyhow::Result<()> {
  let selected = pci::read(device, 22, 2)?;
  let count = pci::read(device, 18, 2)?;
  let result = (|| {
    for index in 0..count {
      pci::write(device, memory, 22, 2, index)?;
      let size = pci::read(device, 24, 2)?;
      let enabled = pci::read(device, 28, 2)?;
      let vector = pci::read(device, 26, 2)?;
      if enabled == 0 {
        eprintln!("Optical queue {index}: disabled, size {size}, vector {vector}");
        continue;
      }
      let available = pci::read(device, 40, 4)? as u64 | (pci::read(device, 44, 4)? as u64) << 32;
      let used = pci::read(device, 48, 4)? as u64 | (pci::read(device, 52, 4)? as u64) << 32;
      let mut published = [0; 4];
      let mut completed = [0; 4];
      memory.read(available, &mut published)?;
      memory.read(used, &mut completed)?;
      eprintln!("Optical queue {index}: size {size}, vector {vector}, available flags {}, index {}, used flags {}, index {}",
        u16::from_le_bytes(published[..2].try_into()?), u16::from_le_bytes(published[2..].try_into()?),
        u16::from_le_bytes(completed[..2].try_into()?), u16::from_le_bytes(completed[2..].try_into()?));
    }
    Ok(())
  })();
  pci::write(device, memory, 22, 2, selected)?;
  result
}
