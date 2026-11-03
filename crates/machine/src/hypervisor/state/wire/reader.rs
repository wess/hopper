use anyhow::ensure;
use zeroize::Zeroizing;

pub(super) struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
  pub(super) fn new(bytes: &'a [u8]) -> Self {
    Self(bytes)
  }

  pub(super) fn array<const N: usize>(&mut self) -> anyhow::Result<[u8; N]> {
    ensure!(self.0.len() >= N, "CPU record is truncated");
    let (value, rest) = self.0.split_at(N);
    self.0 = rest;
    Ok(value.try_into()?)
  }

  pub(super) fn byte(&mut self) -> anyhow::Result<u8> {
    Ok(self.array::<1>()?[0])
  }

  pub(super) fn u16(&mut self) -> anyhow::Result<u16> {
    Ok(u16::from_le_bytes(self.array()?))
  }

  pub(super) fn u32(&mut self) -> anyhow::Result<u32> {
    Ok(u32::from_le_bytes(self.array()?))
  }

  pub(super) fn u64(&mut self) -> anyhow::Result<u64> {
    Ok(u64::from_le_bytes(self.array()?))
  }

  pub(super) fn count(&mut self, expected: usize) -> anyhow::Result<()> {
    ensure!(
      usize::from(self.u16()?) == expected,
      "CPU bank dimensions are incompatible"
    );
    Ok(())
  }

  pub(super) fn list(&mut self, ids: &[u16]) -> anyhow::Result<Zeroizing<Vec<u64>>> {
    self.count(ids.len())?;
    let mut values = Zeroizing::new(Vec::with_capacity(ids.len()));
    for &expected in ids {
      ensure!(
        self.u16()? == expected,
        "CPU system register identity is incompatible"
      );
      values.push(self.u64()?);
    }
    Ok(values)
  }

  pub(super) fn finish(&self) -> anyhow::Result<()> {
    ensure!(self.0.is_empty(), "CPU record has trailing data");
    Ok(())
  }
}
