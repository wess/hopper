use super::{Root, SIZE};
use anyhow::ensure;
use sha2::{Digest, Sha256};
use std::{
  io::{Read, Write},
  sync::atomic::{AtomicU64, Ordering},
};

const MAGIC: &[u8; 8] = b"HOPBANK1";
static NEXT: AtomicU64 = AtomicU64::new(0);

pub(super) fn read(root: &Root) -> anyhow::Result<(Vec<u8>, u64)> {
  let file = root.file("bank", false)?;
  ensure!(
    file.metadata()?.len() == (SIZE + 48) as u64,
    "Invalid variable bank size"
  );
  let mut bytes = Vec::with_capacity(SIZE + 48);
  file.take((SIZE + 49) as u64).read_to_end(&mut bytes)?;
  ensure!(
    bytes.len() == SIZE + 48 && &bytes[..8] == MAGIC,
    "Invalid variable bank framing"
  );
  ensure!(
    Sha256::digest(&bytes[..SIZE + 16])[..] == bytes[SIZE + 16..],
    "Corrupt variable bank"
  );
  Ok((
    bytes[16..SIZE + 16].to_vec(),
    u64::from_le_bytes(bytes[8..16].try_into()?),
  ))
}

pub(super) fn write(root: &Root, bank: &[u8], sequence: u64) -> anyhow::Result<()> {
  ensure!(bank.len() == SIZE, "Invalid variable bank size");
  let path = format!(
    "pending.{}.{}",
    std::process::id(),
    NEXT.fetch_add(1, Ordering::Relaxed)
  );
  let mut output = root.file(&path, true)?;
  let result = (|| -> anyhow::Result<()> {
    let mut checksum = Sha256::new();
    checksum.update(MAGIC);
    checksum.update(sequence.to_le_bytes());
    checksum.update(bank);
    output.write_all(MAGIC)?;
    output.write_all(&sequence.to_le_bytes())?;
    output.write_all(bank)?;
    output.write_all(&checksum.finalize())?;
    output.sync_all()?;
    root.replace(&path, "bank")?;
    root.sync()?;
    Ok(())
  })();
  if result.is_err() {
    root.remove(&path);
  }
  result
}
