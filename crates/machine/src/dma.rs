use anyhow::ensure;

pub trait Memory {
  fn contains(&self, address: u64, length: usize) -> bool;
  fn read(&self, address: u64, bytes: &mut [u8]) -> anyhow::Result<()>;
  fn write(&mut self, address: u64, bytes: &[u8]) -> anyhow::Result<()>;
}

#[derive(Clone, Copy, Debug)]
pub struct Span {
  pub address: u64,
  pub length: u32,
}

pub fn length(spans: &[Span]) -> u64 {
  spans.iter().map(|span| span.length as u64).sum()
}

pub fn read(
  memory: &impl Memory,
  spans: &[Span],
  offset: u64,
  bytes: &mut [u8],
) -> anyhow::Result<()> {
  transfer(spans, offset, bytes.len(), |address, range| {
    memory.read(address, &mut bytes[range])
  })
}

pub fn write(
  memory: &mut impl Memory,
  spans: &[Span],
  offset: u64,
  bytes: &[u8],
) -> anyhow::Result<()> {
  transfer(spans, offset, bytes.len(), |address, range| {
    memory.write(address, &bytes[range])
  })
}

fn transfer(
  spans: &[Span],
  mut offset: u64,
  count: usize,
  mut copy: impl FnMut(u64, std::ops::Range<usize>) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
  ensure!(
    offset
      .checked_add(count as u64)
      .is_some_and(|end| end <= length(spans)),
    "DMA transfer exceeds its buffers"
  );
  if count == 0 {
    return Ok(());
  }
  let mut done = 0;
  for span in spans {
    if offset >= span.length as u64 {
      offset -= span.length as u64;
      continue;
    }
    let size = (span.length as u64 - offset).min((count - done) as u64) as usize;
    let address = span
      .address
      .checked_add(offset)
      .ok_or_else(|| anyhow::anyhow!("DMA address overflow"))?;
    copy(address, done..done + size)?;
    done += size;
    offset = 0;
    if done == count {
      break;
    }
  }
  Ok(())
}
