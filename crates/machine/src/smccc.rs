//! SMCCC entropy services backed by the host's operating-system random source.

pub fn call(
  command: u32,
  argument: u64,
  fill: &mut dyn FnMut(&mut [u8]) -> anyhow::Result<()>,
) -> Option<[u64; 4]> {
  let error = |status: i64| [status as u64, 0, 0, 0];
  match command {
    0x80000000 => Some([0x10001, 0, 0, 0]),
    0x84000050 => Some([0x10000, 0, 0, 0]),
    0x84000051 => Some(error(
      if matches!(argument as u32, 0x84000050..=0x84000053 | 0xc4000053) {
        0
      } else {
        -1
      },
    )),
    0x84000052 => Some([0x31da2d4e, 0x4d9f0764, 0x268edca5, 0xfda98be2]),
    0x84000053 | 0xc4000053 => {
      let width = if command == 0x84000053 { 32 } else { 64 };
      if argument == 0 || argument > width * 3 {
        return Some(error(-2));
      }
      let mut bytes = [0u8; 24];
      if fill(&mut bytes[..argument.div_ceil(8) as usize]).is_err() {
        return Some(error(-3));
      }
      let mut reply = [0u64; 4];
      let mut remaining = argument;
      for (index, chunk) in bytes.chunks_exact((width / 8) as usize).take(3).enumerate() {
        let mut word = [0u8; 8];
        word[..chunk.len()].copy_from_slice(chunk);
        let bits = remaining.min(width);
        let mask = if bits == 64 {
          u64::MAX
        } else {
          (1u64 << bits) - 1
        };
        reply[3 - index] = u64::from_le_bytes(word) & mask;
        remaining -= bits;
      }
      Some(reply)
    }
    _ => None,
  }
}
