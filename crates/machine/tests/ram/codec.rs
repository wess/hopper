use super::{decode, encode, CHUNK};
use sha2::{Digest, Sha256};
use std::io::{Cursor, Read};

fn packet(size: usize) -> Vec<u8> {
  let mut bytes = Vec::from(b"HRM1");
  bytes.extend_from_slice(&0x40000000u64.to_le_bytes());
  bytes.extend_from_slice(&(size as u64).to_le_bytes());
  bytes.extend_from_slice(&(CHUNK as u32).to_le_bytes());
  for offset in (0..size).step_by(CHUNK) {
    let count = CHUNK.min(size - offset);
    let stored = if offset == CHUNK { 0 } else { count };
    bytes.extend_from_slice(&(stored as u32).to_le_bytes());
    bytes.extend(std::iter::repeat_n(0xa5, stored));
  }
  checksum(&mut bytes, false);
  bytes
}

fn checksum(bytes: &mut Vec<u8>, replace: bool) {
  if replace {
    bytes.truncate(bytes.len() - 32);
  }
  let checksum = Sha256::digest(&*bytes);
  bytes.extend_from_slice(&checksum);
}

#[test]
fn sparse_chunks_and_partial_last_chunk_round_trip() -> anyhow::Result<()> {
  let size = CHUNK * 2 + 0x4000;
  let bytes = packet(size);
  for expected in [(0x40004000, size), (0x40000000, size + 0x4000)] {
    assert!(decode(&mut Cursor::new(&bytes), expected, || Ok(())).is_err());
  }
  let saved = decode(&mut Cursor::new(&bytes), (0x40000000, size), || Ok(()))?;
  assert_ne!(
    unsafe {
      libc::mprotect(
        saved.address as *mut libc::c_void,
        size,
        libc::PROT_READ | libc::PROT_WRITE,
      )
    },
    0,
    "Reopened memory permits writable protection"
  );
  assert_eq!(saved.guest(), 0x40000000);
  assert_eq!(saved.size(), size);
  let mut buffer = [1; 8];
  for (offset, expected) in [(0, 0xa5), (CHUNK, 0), (size - 8, 0xa5)] {
    saved.read(offset, &mut buffer)?;
    assert_eq!(buffer, [expected; 8]);
  }
  let mut reopened = Vec::new();
  encode(&saved, &mut reopened, || Ok(()))?;
  assert_eq!(reopened, bytes);
  Ok(())
}

#[test]
fn malformed_dimensions_chunks_damage_and_trailing_bytes_reject() {
  let size = 0x4000;
  let bytes = packet(size);
  for end in [
    0,
    3,
    4,
    12,
    20,
    23,
    24,
    25,
    27,
    28,
    100,
    bytes.len() - 33,
    bytes.len() - 1,
  ] {
    assert!(
      decode(&mut Cursor::new(&bytes[..end]), (0x40000000, size), || Ok(
        ()
      ))
      .is_err()
    );
  }
  for offset in [0, 4, 12, 19, 20, 24, 28, 100, bytes.len() - 1] {
    let mut changed = bytes.clone();
    changed[offset] ^= 1;
    assert!(decode(&mut Cursor::new(changed), (0x40000000, size), || Ok(())).is_err());
  }
  for (offset, value) in [(4, u64::MAX), (12, 0), (12, (64 << 30) + 0x4000)] {
    let mut changed = bytes.clone();
    changed[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    checksum(&mut changed, true);
    assert!(decode(&mut Cursor::new(changed), (0x40000000, size), || Ok(())).is_err());
  }
  let mut extended = bytes.clone();
  extended.push(0);
  assert!(decode(&mut Cursor::new(extended), (0x40000000, size), || Ok(())).is_err());
}

#[test]
fn revocation_partial_io_and_writer_failure_never_return_a_record() -> anyhow::Result<()> {
  let size = CHUNK * 2 + 0x4000;
  let bytes = packet(size);
  for reject in 1..=10 {
    let mut checks = 0;
    let result = decode(&mut Cursor::new(&bytes), (0x40000000, size), || {
      checks += 1;
      anyhow::ensure!(checks != reject, "revoked");
      Ok(())
    });
    assert!(result.is_err());
  }
  let saved = decode(&mut Cursor::new(&bytes), (0x40000000, size), || Ok(()))?;
  let mut full = std::io::Cursor::new([0; 100]);
  assert!(encode(&saved, &mut full, || Ok(())).is_err());
  let mut partial = Vec::new();
  let mut checks = 0;
  assert!(encode(&saved, &mut partial, || {
    checks += 1;
    anyhow::ensure!(checks < 4, "revoked");
    Ok(())
  })
  .is_err());
  assert!(decode(&mut Cursor::new(partial), (0x40000000, size), || Ok(())).is_err());
  let mut fragmented = bytes.as_slice().take(25);
  assert!(decode(&mut fragmented, (0x40000000, size), || Ok(())).is_err());
  Ok(())
}
