use super::*;
use sha2::{Digest, Sha256};
use std::io::{self, Read, Write};

fn encode() -> Vec<u8> {
  let mut ram = Ram::default();
  let saved = snapshot::capture(&display(&mut ram));
  let mut bytes = Vec::new();
  snapshot::wire::encode(&saved, &mut bytes, || Ok(())).unwrap();
  bytes
}

fn rejected(bytes: &[u8]) {
  assert!(snapshot::wire::decode(&mut &*bytes, (640, 480), || Ok(())).is_err());
}

fn checksum(bytes: &mut [u8]) {
  let end = bytes.len() - 32;
  let digest = Sha256::digest(&bytes[..end]);
  bytes[end..].copy_from_slice(&digest);
}

#[test]
fn rejects_corruption_truncation_trailing_bytes_and_invalid_resource_metadata() {
  let bytes = encode();
  for length in [0, 4, 12, 24, bytes.len() - 33, bytes.len() - 1] {
    rejected(&bytes[..length]);
  }
  let mut changed = bytes.clone();
  changed[40] ^= 1;
  rejected(&changed);
  let mut extended = bytes.clone();
  extended.push(0);
  rejected(&extended);
  // resource count, id, width, height and format precede the first pixel allocation.
  for (offset, value) in [(20, 65), (24, 0), (28, 8193), (32, 0), (36, 5)] {
    let mut changed = bytes.clone();
    changed[offset..offset + 4].copy_from_slice(&u32::to_le_bytes(value));
    checksum(&mut changed);
    rejected(&changed);
  }
  // first resource has 16 pixel bytes, then one packed 12-byte backing span.
  for (offset, value) in [(56, 4097), (68, 0)] {
    let mut changed = bytes.clone();
    changed[offset..offset + 4].copy_from_slice(&u32::to_le_bytes(value));
    checksum(&mut changed);
    rejected(&changed);
  }
  let mut duplicate = bytes.clone();
  duplicate[72..76].copy_from_slice(&1u32.to_le_bytes());
  checksum(&mut duplicate);
  rejected(&duplicate);
}

struct BindingReader<'a>(&'a [u8], usize);
impl Read for BindingReader<'_> {
  fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
    self.1 += 1;
    assert!(self.1 <= 3, "foreign binding read past display dimensions");
    self.0.read(output)
  }
}

#[test]
fn foreign_display_binding_rejects_before_pixel_or_metadata_allocation() {
  let bytes = encode();
  let mut input = BindingReader(&bytes, 0);
  assert!(snapshot::wire::decode(&mut input, (800, 600), || Ok(())).is_err());
}

#[test]
fn authorization_revocation_and_io_errors_never_return_a_record() {
  let bytes = encode();
  let mut checks = 0;
  snapshot::wire::decode(&mut bytes.as_slice(), (640, 480), || {
    checks += 1;
    Ok(())
  })
  .unwrap();
  for stop in [1, 10, checks - 1, checks] {
    let mut calls = 0;
    assert!(
      snapshot::wire::decode(&mut bytes.as_slice(), (640, 480), || {
        calls += 1;
        anyhow::ensure!(calls < stop, "revoked");
        Ok(())
      })
      .is_err()
    );
  }
  let mut ram = Ram::default();
  let saved = snapshot::capture(&display(&mut ram));
  let mut checks = 0;
  snapshot::wire::encode(&saved, &mut io::sink(), || {
    checks += 1;
    Ok(())
  })
  .unwrap();
  for stop in [1, 10, checks] {
    let mut calls = 0;
    assert!(snapshot::wire::encode(&saved, &mut io::sink(), || {
      calls += 1;
      anyhow::ensure!(calls < stop, "revoked");
      Ok(())
    })
    .is_err());
  }
  struct Failed;
  impl Write for Failed {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
      Err(io::Error::other("failed"))
    }
    fn flush(&mut self) -> io::Result<()> {
      Ok(())
    }
  }
  assert!(snapshot::wire::encode(&saved, &mut Failed, || Ok(())).is_err());
}

#[test]
fn empty_and_reset_linear_records_remain_usable() {
  let saved = reopen(snapshot::capture(&gpu::create(640, 480).unwrap()));
  let mut ram = Ram::default();
  let restored = snapshot::restore(&saved, &gpu::create(640, 480).unwrap(), &ram).unwrap();
  assert!(gpu::frame(&restored).is_none());
  let mut original = display(&mut ram);
  for (offset, value) in [
    (8, (BASE + 0x3000) as u32),
    (16, 2),
    (20, 2),
    (24, 2),
    (28, 1),
  ] {
    gpu::linear_write(&mut original, &ram, offset, value).unwrap();
  }
  gpu::refresh(&mut original, &ram).unwrap();
  gpu::reset(&mut original);
  let saved = reopen(snapshot::capture(&original));
  let restored = snapshot::restore(&saved, &gpu::create(640, 480).unwrap(), &ram).unwrap();
  assert_eq!(
    gpu::frame(&restored).unwrap().rgba,
    gpu::frame(&original).unwrap().rgba
  );
}

#[test]
fn large_pixel_payloads_use_bounded_stream_io() {
  let mut ram = Ram::default();
  let mut display = gpu::create(640, 480).unwrap();
  command(&mut display, &mut ram, 0x101, &words(&[1, 67, 256, 256]));
  command(
    &mut display,
    &mut ram,
    0x103,
    &words(&[0, 0, 256, 256, 0, 1]),
  );
  struct Stream(Vec<u8>, usize);
  impl Write for Stream {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
      assert!(bytes.len() <= 65536);
      self.0.extend_from_slice(bytes);
      Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
      Ok(())
    }
  }
  impl Read for Stream {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
      assert!(bytes.len() <= 65536);
      let count = bytes.len().min(self.0.len() - self.1);
      bytes[..count].copy_from_slice(&self.0[self.1..self.1 + count]);
      self.1 += count;
      Ok(count)
    }
  }
  let saved = snapshot::capture(&display);
  let mut stream = Stream(Vec::new(), 0);
  snapshot::wire::encode(&saved, &mut stream, || Ok(())).unwrap();
  drop((saved, display));
  assert!(stream.0.len() > 512 * 1024);
  let saved = snapshot::wire::decode(&mut stream, (640, 480), || Ok(())).unwrap();
  let display = snapshot::restore(&saved, &gpu::create(640, 480).unwrap(), &ram).unwrap();
  assert_eq!(gpu::frame(&display).unwrap().rgba, vec![0; 256 * 256 * 4]);
}
