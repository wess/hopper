use anyhow::ensure;
use flate2::bufread::GzDecoder;
use machine::vz::{kernel::header, queue::Check};
use std::io::Read;

pub const MAXIMUM: usize = 128 << 20;

pub fn decode(input: &[u8], check: &Check) -> anyhow::Result<Vec<u8>> {
  check()?;
  ensure!(
    (64..=MAXIMUM).contains(&input.len()),
    "Installer kernel exceeds bounds"
  );
  if header(input).is_ok() {
    let output = input.to_vec();
    check()?;
    return Ok(output);
  }
  let compressed = if input.starts_with(b"\x1f\x8b") {
    input
  } else {
    ensure!(
      &input[..8] == b"MZ\0\0zimg",
      "Unsupported ARM64 installer kernel format"
    );
    ensure!(input[16..24] == [0; 8], "Unsupported EFI kernel header");
    let offset = u32::from_le_bytes(input[8..12].try_into()?) as usize;
    let size = u32::from_le_bytes(input[12..16].try_into()?) as usize;
    ensure!(
      offset >= 64
        && size > 0
        && offset
          .checked_add(size)
          .is_some_and(|end| end <= input.len()),
      "EFI kernel payload exceeds bounds"
    );
    &input[offset..offset + size]
  };
  let gzip = compressed.starts_with(b"\x1f\x8b");
  let zstd = compressed.starts_with(b"\x28\xb5\x2f\xfd");
  ensure!(gzip || zstd, "Unsupported EFI kernel compression");
  if &input[..8] == b"MZ\0\0zimg" {
    ensure!(
      (gzip && &input[24..29] == b"gzip\0") || (zstd && &input[24..29] == b"zstd\0"),
      "EFI compression header disagrees with payload"
    );
  }
  let mut decoder: Box<dyn Read + '_> = if gzip {
    Box::new(GzDecoder::new(compressed))
  } else {
    let mut decoder = zstd::stream::read::Decoder::new(compressed)?.single_frame();
    decoder.window_log_max(27)?;
    Box::new(decoder)
  };
  let mut output = Vec::new();
  let mut chunk = [0; 64 << 10];
  loop {
    check()?;
    let count = decoder.read(&mut chunk)?;
    if count == 0 {
      break;
    }
    ensure!(
      output.len() + count <= MAXIMUM,
      "Uncompressed kernel exceeds bounds"
    );
    output.extend_from_slice(&chunk[..count]);
  }
  header(&output)?;
  check()?;
  Ok(output)
}
