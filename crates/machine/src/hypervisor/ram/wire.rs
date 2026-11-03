//! HRM1 streams immutable memory in 64 KiB chunks, omitting zero chunks.
//! A VM-bound manifest and private atomic publication remain the caller's responsibility.

use super::Capture;
use anyhow::{ensure, Context};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use zeroize::Zeroizing;

#[cfg(test)]
#[path = "../../../tests/ram/codec.rs"]
mod tests;

const CHUNK: usize = 64 * 1024;
const LIMIT: u64 = 64 << 30;
pub const MAX: u64 = LIMIT + (LIMIT / CHUNK as u64) * 4 + 56;

fn dimensions(guest: u64, size: u64) -> anyhow::Result<usize> {
  let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
  ensure!(page > 0, "Cannot determine memory record page size");
  ensure!(
    (page as u64..=LIMIT).contains(&size)
      && size.is_multiple_of(page as u64)
      && guest.is_multiple_of(page as u64)
      && guest.checked_add(size).is_some_and(|end| end <= 1 << 40),
    "Memory record dimensions are invalid"
  );
  usize::try_from(size).context("Memory record exceeds host address space")
}

/// Stream after releasing the VM pause; the capture owns an immutable COW cut.
/// On any error output must remain unpublished, even if all bytes were written.
pub fn encode(
  saved: &Capture,
  output: &mut impl Write,
  mut authorize: impl FnMut() -> anyhow::Result<()>,
) -> anyhow::Result<()> {
  authorize()?;
  dimensions(saved.guest(), saved.size() as u64)?;
  let mut header = [0; 24];
  header[..4].copy_from_slice(b"HRM1");
  header[4..12].copy_from_slice(&saved.guest().to_le_bytes());
  header[12..20].copy_from_slice(&(saved.size() as u64).to_le_bytes());
  header[20..24].copy_from_slice(&(CHUNK as u32).to_le_bytes());
  let mut digest = Sha256::new();
  digest.update(header);
  output.write_all(&header)?;
  let mut bytes = Zeroizing::new(vec![0; CHUNK]);
  for offset in (0..saved.size()).step_by(CHUNK) {
    authorize()?;
    let count = CHUNK.min(saved.size() - offset);
    saved.read(offset, &mut bytes[..count])?;
    let stored = if bytes[..count].iter().all(|byte| *byte == 0) {
      0
    } else {
      count
    };
    let length = (stored as u32).to_le_bytes();
    digest.update(length);
    digest.update(&bytes[..stored]);
    output.write_all(&length)?;
    output.write_all(&bytes[..stored])?;
    authorize()?;
  }
  output.write_all(&digest.finalize())?;
  authorize()
}

/// Read bounded chunks into private anonymous memory, then seal it read-only.
/// No guest hardware is allocated. Read I/O must itself be bounded by the caller.
pub fn decode(
  input: &mut impl Read,
  expected: (u64, usize),
  mut authorize: impl FnMut() -> anyhow::Result<()>,
) -> anyhow::Result<Capture> {
  authorize()?;
  let mut header = [0; 24];
  input.read_exact(&mut header)?;
  authorize()?;
  ensure!(
    &header[..4] == b"HRM1",
    "Memory record format is unsupported"
  );
  let guest = u64::from_le_bytes(header[4..12].try_into()?);
  let size = dimensions(guest, u64::from_le_bytes(header[12..20].try_into()?))?;
  ensure!(
    (guest, size) == expected,
    "Memory record does not match the authorized bank"
  );
  ensure!(
    u32::from_le_bytes(header[20..24].try_into()?) as usize == CHUNK,
    "Memory record chunk size is unsupported"
  );
  let address = unsafe {
    libc::mmap(
      std::ptr::null_mut(),
      size,
      libc::PROT_READ | libc::PROT_WRITE,
      libc::MAP_PRIVATE | libc::MAP_ANON,
      -1,
      0,
    )
  };
  ensure!(
    address != libc::MAP_FAILED,
    "Allocate memory record: {}",
    std::io::Error::last_os_error()
  );
  let saved = Capture {
    address: address as u64,
    size,
    guest,
  };
  ensure!(!address.is_null(), "Memory record mapping returned null");
  let mut digest = Sha256::new();
  digest.update(header);
  let mut bytes = Zeroizing::new(vec![0; CHUNK]);
  for offset in (0..size).step_by(CHUNK) {
    authorize()?;
    let mut length = [0; 4];
    input.read_exact(&mut length)?;
    let stored = u32::from_le_bytes(length) as usize;
    let count = CHUNK.min(size - offset);
    ensure!(
      stored == 0 || stored == count,
      "Memory record chunk length is invalid"
    );
    digest.update(length);
    if stored != 0 {
      input.read_exact(&mut bytes[..stored])?;
      digest.update(&bytes[..stored]);
      unsafe {
        std::ptr::copy_nonoverlapping(
          bytes.as_ptr(),
          (saved.address as *mut u8).add(offset),
          stored,
        )
      };
    }
    authorize()?;
  }
  let mut checksum = [0; 32];
  input.read_exact(&mut checksum)?;
  ensure!(
    digest.finalize()[..] == checksum,
    "Memory record checksum is invalid"
  );
  let mut extra = [0; 1];
  ensure!(
    input.read(&mut extra)? == 0,
    "Memory record has trailing bytes"
  );
  authorize()?;
  let status =
    unsafe { super::mach_vm_protect(super::mach_task_self_, saved.address, size as u64, 1, 1) };
  ensure!(status == 0, "Protect reopened memory: Mach error {status}");
  authorize()?;
  Ok(saved)
}
