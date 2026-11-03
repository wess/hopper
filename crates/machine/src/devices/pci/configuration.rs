use super::Function;

impl Function {
  pub(crate) fn configuration(&self) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut config = self.config;
    config[4..8].fill(0);
    config[0x3c] = 0;
    if let Some(offset) = self.msix {
      config[offset + 3] = 0;
    }
    let mut hash = Sha256::new();
    hash.update(b"hopper.pci.configuration.v1");
    for offset in [
      self.capability,
      self.next_capability,
      self.msix.unwrap_or(0xffff),
    ] {
      hash.update((offset as u64).to_le_bytes());
    }
    hash.update(config);
    hash.update((self.bars.len() as u64).to_le_bytes());
    for bar in &self.bars {
      hash.update((bar.slot as u64).to_le_bytes());
      hash.update(bar.size.to_le_bytes());
      hash.update([bar.wide as u8]);
    }
    hash.finalize().into()
  }
}
