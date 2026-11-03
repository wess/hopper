//! HCS1 architectural CPU records, bounded and checksummed; not a VM snapshot.
//! owned encoded buffers erase on drop and must be stored in private files.

mod reader;

#[cfg(test)]
#[path = "../../../tests/cpu/codec.rs"]
mod tests;

use super::{debug, features, State, INTERRUPTS, SYSTEM};
use anyhow::ensure;
use reader::Reader;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

pub const MAX: usize = 4096;

/// Register IDs and bank dimensions form the schema; changes require a new format.
/// The checksum detects damage, not an unauthorized author. A VM-bound manifest is separate.
pub fn encode(state: &State) -> anyhow::Result<Zeroizing<Vec<u8>>> {
  features::validate(&state.features)?;
  state.debug.validate(state.features[5])?;
  ensure!(
    state.system.len() == SYSTEM.len() && state.interrupts.len() == INTERRUPTS.len(),
    "CPU state dimensions are incompatible"
  );
  let mut bytes = Zeroizing::new(Vec::with_capacity(MAX));
  bytes.extend_from_slice(b"HCS1\0\0\0\0");
  list(&mut bytes, &features::REGISTERS, &state.features);
  bytes.extend_from_slice(&35u16.to_le_bytes());
  for (index, value) in state.registers.iter().enumerate() {
    bytes.extend_from_slice(&(index as u32).to_le_bytes());
    bytes.extend_from_slice(&value.to_le_bytes());
  }
  bytes.extend_from_slice(&32u16.to_le_bytes());
  for (index, value) in state.vectors.iter().enumerate() {
    bytes.extend_from_slice(&(index as u32).to_le_bytes());
    bytes.extend_from_slice(value);
  }
  list(&mut bytes, SYSTEM, &state.system);
  list(&mut bytes, INTERRUPTS, &state.interrupts);
  let debug = state.debug.values();
  bytes.extend_from_slice(&(debug.len() as u16).to_le_bytes());
  for &(register, value) in debug {
    bytes.extend_from_slice(&register.to_le_bytes());
    bytes.extend_from_slice(&value.to_le_bytes());
  }
  bytes.extend_from_slice(&state.timer_offset.to_le_bytes());
  bytes.push(u8::from(state.timer_mask) | (u8::from(state.serror) << 1));
  ensure!(bytes.len() + 32 <= MAX, "CPU state exceeds its bound");
  let length = (bytes.len() - 8) as u32;
  bytes[4..8].copy_from_slice(&length.to_le_bytes());
  let checksum = Sha256::digest(&*bytes);
  bytes.extend_from_slice(&checksum);
  Ok(bytes)
}

fn list(bytes: &mut Vec<u8>, ids: &[u16], values: &[u64]) {
  bytes.extend_from_slice(&(ids.len() as u16).to_le_bytes());
  for (&register, &value) in ids.iter().zip(values) {
    bytes.extend_from_slice(&register.to_le_bytes());
    bytes.extend_from_slice(&value.to_le_bytes());
  }
}

/// No hardware is allocated or changed. Rejects incompatible bank/register identities,
/// unsupported vectors, unknown flags, extra/truncated data and invalid integrity.
pub fn decode(bytes: &[u8]) -> anyhow::Result<State> {
  ensure!(
    (40..=MAX).contains(&bytes.len()),
    "CPU record size is invalid"
  );
  ensure!(&bytes[..4] == b"HCS1", "CPU record format is unsupported");
  let length = u32::from_le_bytes(bytes[4..8].try_into()?) as usize;
  ensure!(length == bytes.len() - 40, "CPU record length is invalid");
  let end = bytes.len() - 32;
  ensure!(
    Sha256::digest(&bytes[..end]).as_slice() == &bytes[end..],
    "CPU record checksum is invalid"
  );
  let mut reader = Reader::new(&bytes[8..end]);
  let values = reader.list(&features::REGISTERS)?;
  let features: [u64; 12] = values.as_slice().try_into()?;
  features::validate(&features)?;
  reader.count(35)?;
  let mut registers = Zeroizing::new([0; 35]);
  for (index, value) in registers.iter_mut().enumerate() {
    ensure!(
      reader.u32()? == index as u32,
      "CPU general register identity is invalid"
    );
    *value = reader.u64()?;
  }
  reader.count(32)?;
  let mut vectors = Zeroizing::new([[0; 16]; 32]);
  for (index, value) in vectors.iter_mut().enumerate() {
    ensure!(
      reader.u32()? == index as u32,
      "CPU vector register identity is invalid"
    );
    *value = reader.array()?;
  }
  let mut system = reader.list(SYSTEM)?;
  let mut interrupts = reader.list(INTERRUPTS)?;
  let count = reader.u16()? as usize;
  ensure!(count <= 66, "CPU debug bank exceeds its bound");
  let mut values = Zeroizing::new(Vec::with_capacity(count));
  for _ in 0..count {
    values.push((reader.u16()?, reader.u64()?));
  }
  let debug = debug::Debug::from_values(features[5], std::mem::take(&mut *values))?;
  let timer_offset = reader.u64()?;
  let flags = reader.byte()?;
  ensure!(flags & !3 == 0, "CPU record flags are unsupported");
  reader.finish()?;
  Ok(State {
    features,
    debug,
    registers: *registers,
    vectors: *vectors,
    system: std::mem::take(&mut *system),
    interrupts: std::mem::take(&mut *interrupts),
    timer_offset,
    timer_mask: flags & 1 != 0,
    serror: flags & 2 != 0,
  })
}
