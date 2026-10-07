use machine::dma::Memory;

pub const BASE: u64 = 0x40000000;

pub struct Ram(pub Vec<u8>);

impl Default for Ram {
  fn default() -> Self {
    Self(vec![0; 0x10000])
  }
}

impl Memory for Ram {
  fn contains(&self, address: u64, length: usize) -> bool {
    address
      .checked_sub(BASE)
      .and_then(|offset| offset.checked_add(length as u64))
      .is_some_and(|end| end <= self.0.len() as u64)
  }

  fn read(&self, address: u64, bytes: &mut [u8]) -> anyhow::Result<()> {
    anyhow::ensure!(self.contains(address, bytes.len()), "Read outside test RAM");
    let start = (address - BASE) as usize;
    bytes.copy_from_slice(&self.0[start..start + bytes.len()]);
    Ok(())
  }

  fn write(&mut self, address: u64, bytes: &[u8]) -> anyhow::Result<()> {
    anyhow::ensure!(
      self.contains(address, bytes.len()),
      "Write outside test RAM"
    );
    let start = (address - BASE) as usize;
    self.0[start..start + bytes.len()].copy_from_slice(bytes);
    Ok(())
  }
}

pub fn descriptor(ram: &mut Ram, index: u16, address: u64, length: u32, flags: u16, next: u16) {
  let start = 16 * index as usize;
  ram.0[start..start + 8].copy_from_slice(&address.to_le_bytes());
  ram.0[start + 8..start + 12].copy_from_slice(&length.to_le_bytes());
  ram.0[start + 12..start + 14].copy_from_slice(&flags.to_le_bytes());
  ram.0[start + 14..start + 16].copy_from_slice(&next.to_le_bytes());
}
